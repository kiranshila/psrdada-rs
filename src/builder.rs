use crate::{
    client::HduClient,
    errors::{PsrdadaError, PsrdadaResult},
};
use psrdada_sys::*;
use tracing::{debug, error, warn};

#[derive(Debug)]
pub struct HduClientBuilder {
    key: i32,
    log_name: String,
    // Default things from Psrdada
    num_bufs: Option<u64>,
    buf_size: Option<u64>,
    num_headers: Option<u64>,
    header_size: Option<u64>,
    // Behavior flags
    lock: Option<bool>,
}

impl HduClientBuilder {
    pub fn new(key: i32, log_name: &str) -> Self {
        Self {
            key,
            log_name: log_name.to_string(),
            num_bufs: None,
            buf_size: None,
            num_headers: None,
            header_size: None,
            lock: None,
        }
    }

    pub fn num_bufs(mut self, value: u64) -> Self {
        self.num_bufs = Some(value);
        self
    }

    pub fn buf_size(mut self, value: u64) -> Self {
        self.buf_size = Some(value);
        self
    }

    pub fn num_headers(mut self, value: u64) -> Self {
        self.num_headers = Some(value);
        self
    }

    pub fn header_size(mut self, value: u64) -> Self {
        self.header_size = Some(value);
        self
    }

    pub fn lock(mut self, value: bool) -> Self {
        self.lock = Some(value);
        self
    }

    #[tracing::instrument]
    /// Builder for DadaDB
    /// Buffer size will default to 4x of 128*Page Size
    /// Header size will default to 8x of Page Size
    pub fn build(self) -> PsrdadaResult<HduClient> {
        // Unpack the things we need, defaulting as necessary
        let key = self.key;
        let log_name = &self.log_name;
        let num_bufs = self.num_bufs.unwrap_or(4);
        let buf_size = self.buf_size.unwrap_or((page_size::get() as u64) * 128);
        let num_headers = self.num_headers.unwrap_or(8);
        let header_size = self.header_size.unwrap_or(page_size::get() as u64);
        let lock = self.lock.unwrap_or(false);

        // Create data block, setting readers to 1 (a la mpsc)
        // We'll use create_work to deal with cuda-able stuff
        debug!("Creating data ringbuffer");
        let mut data = Default::default();
        unsafe {
            // Safety: Catch the error
            if ipcbuf_create_work(
                &mut data, self.key, num_bufs, buf_size, 1, -1, // No CUDA for now
            ) != 0
            {
                error!("Error creating data ringbuffer");
                return Err(PsrdadaError::HDUInitError);
            }
        }

        // Create header block
        debug!("Creating header ringbuffer");
        let mut header = Default::default();
        unsafe {
            // Safety: Catch the Error, destroy data if we fail so we don't leak memory
            if ipcbuf_create(&mut header, self.key + 1, num_headers, header_size, 1) != 0 {
                error!("Error creating header ringbuffer");
                // We're kinda SOL if this happens
                if ipcbuf_destroy(&mut data) != 0 {
                    error!("Error destroying data ringbuffer");
                    return Err(PsrdadaError::HDUDestroyError);
                }
                return Err(PsrdadaError::HDUInitError);
            }
        }

        // Lock if required, teardown everything if we fail
        if lock {
            debug!("Locking both ring and data buffers in shared memory");
            unsafe {
                if ipcbuf_lock(&mut data) != 0 {
                    error!("Error locking data rinngbuffer");
                    if ipcbuf_destroy(&mut data) != 0 {
                        error!("Error destroying data ringbuffer");
                        return Err(PsrdadaError::HDUDestroyError);
                    }
                    if ipcbuf_destroy(&mut header) != 0 {
                        error!("Error destroying header ringbuffer");
                        return Err(PsrdadaError::HDUDestroyError);
                    }
                    return Err(PsrdadaError::HDUShmemLockError);
                }

                if ipcbuf_lock(&mut header) != 0 {
                    error!("Error locking header ringbuffer");
                    if ipcbuf_destroy(&mut data) != 0 {
                        error!("Error destroying data ringbuffer");
                        return Err(PsrdadaError::HDUDestroyError);
                    }
                    if ipcbuf_destroy(&mut header) != 0 {
                        error!("Error destroying header ringbuffer");
                        return Err(PsrdadaError::HDUDestroyError);
                    }
                    return Err(PsrdadaError::HDUShmemLockError);
                }
            }
        }

        // Now we construct our HDU to these buffers we created
        let hdu = HduClient::build(key, log_name)?;
        // Return built result
        Ok(hdu)
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::next_key;
    use test_log::test;

    use super::*;

    #[test]
    fn test_construct_hdu() {
        let key = next_key();
        let _client = HduClientBuilder::new(key, "test").build().unwrap();
    }
}
