use crate::{
    errors::{PsrdadaError, PsrdadaResult},
    logging::create_stderr_log,
};
use psrdada_sys::*;
use tracing::{debug, error, warn};

#[derive(Debug)]
/// The struct that stores the Header + Data unit (HDU)
pub struct HduClient {
    key: i32,
    pub(crate) hdu: *mut dada_hdu,
    allocated: bool,
}

impl HduClient {
    #[tracing::instrument]
    /// Internal method used by builder (we know we allocated it)
    pub(crate) fn build(key: i32, log_name: &str) -> PsrdadaResult<Self> {
        let hdu = Self::connect(key, log_name)?;
        Ok(Self {
            key,
            hdu,
            allocated: true,
        })
    }

    #[tracing::instrument]
    /// Construct a new HduClient and connect to existing ring buffers
    pub fn new(key: i32, log_name: &str) -> PsrdadaResult<Self> {
        let hdu = Self::connect(key, log_name)?;
        Ok(Self {
            key,
            hdu,
            allocated: false,
        })
    }

    #[tracing::instrument]
    /// Internal method to actually build and connect
    fn connect(key: i32, log_name: &str) -> PsrdadaResult<*mut dada_hdu> {
        debug!(key, "Connecting to dada buffer");
        // Create the log to stderr with `log_name`
        let mut log = create_stderr_log(log_name)?;
        unsafe {
            let hdu = dada_hdu_create(&mut log);
            // Set the key
            dada_hdu_set_key(hdu, key);
            // Try to connect
            if dada_hdu_connect(hdu) != 0 {
                error!(key, "Could not connect to dada buffer");
                return Err(PsrdadaError::HDUInitError);
            }
            debug!("Connected!");
            Ok(hdu)
        }
    }

    #[tracing::instrument]
    /// Disconnect an existing HduClient
    fn disconnect(&mut self) -> PsrdadaResult<()> {
        debug!("Disconnecting from dada buffer");
        unsafe {
            if dada_hdu_disconnect(self.hdu) != 0 {
                error!("Could not disconnect from HDU");
                return Err(PsrdadaError::HDUDisconnectError);
            }
            dada_hdu_destroy(self.hdu);
        }
        Ok(())
    }

    #[tracing::instrument]
    /// Grab the data buffer size in bytes from a connected HduClient
    pub fn data_buf_size(&self) -> PsrdadaResult<usize> {
        unsafe {
            let size = ipcbuf_get_bufsz((*self.hdu).data_block as *mut ipcbuf_t);
            Ok(size as usize)
        }
    }

    #[tracing::instrument]
    /// Grab the header buffer size in bytes from a connected HduClient
    pub fn header_buf_size(&self) -> PsrdadaResult<usize> {
        unsafe {
            let size = ipcbuf_get_bufsz((*self.hdu).header_block);
            Ok(size as usize)
        }
    }

    #[tracing::instrument]
    /// Grab the number of data buffers in the ring from a connected HduClient
    pub fn data_buf_count(&self) -> PsrdadaResult<u64> {
        unsafe {
            let size = ipcbuf_get_nbufs((*self.hdu).data_block as *mut ipcbuf_t);
            Ok(size)
        }
    }

    #[tracing::instrument]
    /// Grab the number of header buffers in the ring from a connected HduClient
    pub fn header_buf_count(&self) -> PsrdadaResult<u64> {
        unsafe {
            let size = ipcbuf_get_nbufs((*self.hdu).header_block);
            Ok(size)
        }
    }

    #[tracing::instrument]
    /// Register the Hdu buffers as GPU pinned memory
    pub fn cuda_register(&self) -> PsrdadaResult<()> {
        unsafe {
            if dada_cuda_dbregister(self.hdu) != 0 {
                error!("Failed to register buffers as GPU pinned memory");
                return Err(PsrdadaError::GpuError);
            }
        }
        Ok(())
    }

    pub(crate) fn data_buf(&self) -> *const ipcbuf_t {
        unsafe { &(*(*self.hdu).data_block).buf as *const ipcbuf_t }
    }
}

impl Drop for HduClient {
    fn drop(&mut self) {
        self.disconnect().expect("Disconnecting shouldn't fail");
        if self.allocated {
            debug!("Tearing down the data we allocated");
            unsafe {
                let mut ptr = Default::default();
                // Destroy data
                ipcbuf_connect(&mut ptr, self.key);
                if ipcbuf_destroy(&mut ptr) != 0 {
                    error!("Error destroying data buffer");
                }
                // Destroy header
                ipcbuf_connect(&mut ptr, self.key + 1);
                if ipcbuf_destroy(&mut ptr) != 0 {
                    error!("Error destroying header buffer");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::HduClientBuilder;
    use crate::tests::next_key;
    use test_log::test;

    #[test]
    fn test_connect() {
        let key = next_key();
        let _client = HduClientBuilder::new(key, "test").build().unwrap();
        let _connected = HduClient::new(key, "test").unwrap();
    }

    #[test]
    fn test_sizing() {
        let key = next_key();
        let client = HduClientBuilder::new(key, "test")
            .num_bufs(1)
            .buf_size(128)
            .num_headers(4)
            .header_size(64)
            .build()
            .unwrap();
        assert_eq!(client.data_buf_size().unwrap(), 128);
        assert_eq!(client.data_buf_count().unwrap(), 1);
        assert_eq!(client.header_buf_count().unwrap(), 4);
        assert_eq!(client.header_buf_size().unwrap(), 64);
    }
}
