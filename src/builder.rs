//! Builder-pattern implementation of creating psrdada buffers

use psrdada_sys::*;
use tracing::{debug, error, warn};

use crate::{
    client::DadaClient,
    errors::{PsrdadaError, PsrdadaResult},
};

#[derive(Debug)]
pub struct DadaClientBuilder {
    key: i32,
    // Default things from Psrdada
    num_bufs: Option<u64>,
    buf_size: Option<u64>,
    num_headers: Option<u64>,
    header_size: Option<u64>,
    // Behavior flags
    lock: Option<bool>,
    page: Option<bool>,
}

impl DadaClientBuilder {
    /// Create a new builder with a given `key`
    pub fn new(key: i32) -> Self {
        Self {
            key,
            num_bufs: None,
            buf_size: None,
            num_headers: None,
            header_size: None,
            lock: None,
            page: None,
        }
    }

    /// Number of data blocks
    pub fn num_bufs(mut self, value: u64) -> Self {
        self.num_bufs = Some(value);
        self
    }

    /// Size in bytes of each data block
    pub fn buf_size(mut self, value: u64) -> Self {
        self.buf_size = Some(value);
        self
    }

    /// Number of header blocks
    pub fn num_headers(mut self, value: u64) -> Self {
        self.num_headers = Some(value);
        self
    }

    /// Size in bytes of each header block
    pub fn header_size(mut self, value: u64) -> Self {
        self.header_size = Some(value);
        self
    }

    /// Lock the resulting buffers in shared memory
    pub fn lock(mut self, value: bool) -> Self {
        self.lock = Some(value);
        self
    }

    /// Page the resulting buffers in RAM
    pub fn page(mut self, value: bool) -> Self {
        self.page = Some(value);
        self
    }

    #[tracing::instrument]
    /// Builder for DadaClient
    /// Buffer size will default to 4x of 128*Page Size
    /// Header size will default to 8x of Page Size
    pub fn build(self) -> PsrdadaResult<DadaClient> {
        // Unpack the things we need, defaulting as necessary
        let num_bufs = self.num_bufs.unwrap_or(4);
        let buf_size = self.buf_size.unwrap_or((page_size::get() as u64) * 128);
        let num_headers = self.num_headers.unwrap_or(8);
        let header_size = self.header_size.unwrap_or(page_size::get() as u64);
        let lock = self.lock.unwrap_or(false);
        let page = self.page.unwrap_or(false);

        // Create data block, setting readers to 1 (a la mpsc)
        debug!("Creating data ringbuffer");
        let data = Box::into_raw(Box::default());
        unsafe {
            // Safety: Catch the error, no cuda device
            if ipcbuf_create_work(data, self.key, num_bufs, buf_size, 1, -1) != 0 {
                error!("Error creating data ringbuffer");
                return Err(PsrdadaError::DadaInitError);
            }
        }

        // Create header block
        debug!("Creating header ringbuffer");
        let header = Box::into_raw(Box::default());
        unsafe {
            // Safety: Catch the Error, destroy data if we fail so we don't leak memory
            if ipcbuf_create(header, self.key + 1, num_headers, header_size, 1) != 0 {
                error!("Error creating header ringbuffer");
                // We're kinda SOL if this happens
                if ipcbuf_destroy(data) != 0 {
                    error!("Error destroying data ringbuffer");
                    return Err(PsrdadaError::DadaDestroyError);
                }
                return Err(PsrdadaError::DadaInitError);
            }
        }

        // Lock if required, teardown everything if we fail
        if lock {
            debug!("Locking both ring and data buffers in shared memory");
            unsafe {
                if ipcbuf_lock(data) != 0 {
                    error!("Error locking data rinngbuffer");
                    if ipcbuf_destroy(data) != 0 {
                        error!("Error destroying data ringbuffer");
                        return Err(PsrdadaError::DadaDestroyError);
                    }
                    if ipcbuf_destroy(header) != 0 {
                        error!("Error destroying header ringbuffer");
                        return Err(PsrdadaError::DadaDestroyError);
                    }
                    return Err(PsrdadaError::DadaShmemLockError);
                }

                if ipcbuf_lock(header) != 0 {
                    error!("Error locking header ringbuffer");
                    if ipcbuf_destroy(data) != 0 {
                        error!("Error destroying data ringbuffer");
                        return Err(PsrdadaError::DadaDestroyError);
                    }
                    if ipcbuf_destroy(header) != 0 {
                        error!("Error destroying header ringbuffer");
                        return Err(PsrdadaError::DadaDestroyError);
                    }
                    return Err(PsrdadaError::DadaShmemLockError);
                }
            }
        }

        // Page if required, teardown everything if we fail
        if page {
            debug!("Paging both ring and data buffers in RAM");
            unsafe {
                if ipcbuf_page(data) != 0 {
                    error!("Error locking data rinngbuffer");
                    if ipcbuf_destroy(data) != 0 {
                        error!("Error destroying data ringbuffer");
                        return Err(PsrdadaError::DadaDestroyError);
                    }
                    if ipcbuf_destroy(header) != 0 {
                        error!("Error destroying header ringbuffer");
                        return Err(PsrdadaError::DadaDestroyError);
                    }
                    return Err(PsrdadaError::DadaShmemLockError);
                }

                if ipcbuf_page(header) != 0 {
                    error!("Error locking header ringbuffer");
                    if ipcbuf_destroy(data) != 0 {
                        error!("Error destroying data ringbuffer");
                        return Err(PsrdadaError::DadaDestroyError);
                    }
                    if ipcbuf_destroy(header) != 0 {
                        error!("Error destroying header ringbuffer");
                        return Err(PsrdadaError::DadaDestroyError);
                    }
                    return Err(PsrdadaError::DadaShmemLockError);
                }
            }
        }

        // Now we construct our client with these buffers we created
        // Safety: We just constructed these pointers and haven't shared them
        let client = unsafe { DadaClient::build(data, header) }?;
        // Return built result
        Ok(client)
    }
}

#[cfg(test)]
mod tests {
    use test_log::test;

    use super::*;
    use crate::tests::next_key;

    #[test]
    fn test_construct_client() {
        let key = next_key();
        let _client = DadaClientBuilder::new(key).build().unwrap();
    }
}
