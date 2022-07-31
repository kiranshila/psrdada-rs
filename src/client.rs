use std::ffi::c_void;

use crate::errors::{PsrdadaError, PsrdadaResult};
use psrdada_sys::*;
use tracing::{debug, error, warn};

#[derive(Debug)]
/// The struct that stores the Header + Data ringbuffers
pub struct DadaClient {
    allocated: bool,
    pub(crate) data_buf: *mut ipcbuf_t,
    pub(crate) header_buf: *mut ipcbuf_t,
}

impl DadaClient {
    #[tracing::instrument]
    /// Internal method used by builder (we know we allocated it)
    pub(crate) fn build(data_buf: *mut ipcbuf_t, header_buf: *mut ipcbuf_t) -> PsrdadaResult<Self> {
        let mut s = Self {
            data_buf,
            header_buf,
            allocated: true,
        };
        s.cuda_register()?;
        s.reset()?;
        Ok(s)
    }

    #[tracing::instrument]
    /// Construct a new DadaClient and connect to existing ring buffers
    pub fn new(key: i32) -> PsrdadaResult<Self> {
        let (data_buf, header_buf) = Self::connect(key)?;
        let mut s = Self {
            data_buf,
            header_buf,
            allocated: false,
        };
        s.cuda_register()?;
        s.reset()?;
        Ok(s)
    }

    #[tracing::instrument]
    /// Internal method to actually build and connect
    fn connect(key: i32) -> PsrdadaResult<(*mut ipcbuf_t, *mut ipcbuf_t)> {
        debug!(key, "Connecting to dada buffer");
        unsafe {
            let data_buf = Box::into_raw(Box::new(Default::default()));
            if ipcbuf_connect(data_buf, key) != 0 {
                error!(key, "Could not connect to data buffer");
                return Err(PsrdadaError::DadaInitError);
            }
            let header_buf = Box::into_raw(Box::new(Default::default()));
            if ipcbuf_connect(header_buf, key + 1) != 0 {
                error!(key, "Could not connect to header buffer");
                return Err(PsrdadaError::DadaInitError);
            }
            debug!("Connected!");
            Ok((data_buf, header_buf))
        }
    }

    #[tracing::instrument]
    /// Disconnect an existing DadaClient
    fn disconnect(&mut self) -> PsrdadaResult<()> {
        debug!("Disconnecting from dada buffer");
        unsafe {
            if ipcbuf_disconnect(self.data_buf) != 0 {
                error!("Could not disconnect from data buffer");
                return Err(PsrdadaError::DadaDisconnectError);
            }
            if ipcbuf_disconnect(self.header_buf) != 0 {
                error!("Could not disconnect from header buffer");
                return Err(PsrdadaError::DadaDisconnectError);
            }
        }
        Ok(())
    }

    #[tracing::instrument]
    /// Grab the data buffer size in bytes from a connected DadaClient
    pub fn data_buf_size(&self) -> usize {
        unsafe { ipcbuf_get_bufsz(self.data_buf) as usize }
    }

    #[tracing::instrument]
    /// Grab the header buffer size in bytes from a connected DadaClient
    pub fn header_buf_size(&self) -> usize {
        unsafe { ipcbuf_get_bufsz(self.header_buf) as usize }
    }

    #[tracing::instrument]
    /// Grab the number of data buffers in the ring from a connected DadaClient
    pub fn data_buf_count(&self) -> usize {
        unsafe { ipcbuf_get_nbufs(self.data_buf) as usize }
    }

    #[tracing::instrument]
    /// Grab the number of header buffers in the ring from a connected DadaClient
    pub fn header_buf_count(&self) -> usize {
        unsafe { ipcbuf_get_nbufs(self.header_buf) as usize }
    }

    #[tracing::instrument]
    /// Register the data buffer as GPU pinned memory (the header buffer is on the CPU (hopefully))
    /// We do this on construction and should be a nop for CPU memory
    fn cuda_register(&self) -> PsrdadaResult<()> {
        unsafe {
            // Ensure that the data blocks are shmem locked
            if ipcbuf_lock(self.data_buf) != 0 {
                warn!("Error locking data buf in shared memory - try rerunning as su");
                return Ok(());
            }

            // Don't register buffers if they reside on the  device
            // Device num is -1 if they are on the CPU (in which case we need to register them)
            if ipcbuf_get_device(self.data_buf) >= 0 {
                // Nothing to do!
                return Ok(());
            }

            let bufsz = self.data_buf_size();
            let nbufs = self.data_buf_count();

            // Lock each data buffer block as CUDA memory
            for buf_id in 0..nbufs {
                let block = std::slice::from_raw_parts((*self.data_buf).buffer, nbufs)[buf_id];
                // Check for cudaSuccess (0)
                if cudaHostRegister(block as *mut c_void, bufsz as u64, 0) != 0 {
                    error!("Error registering GPU memory");
                    return Err(PsrdadaError::GpuError);
                }
            }
            debug!("Registered data block as GPU memory!");
        }
        Ok(())
    }

    #[tracing::instrument]
    /// Reset the state of everything
    pub fn reset(&mut self) -> PsrdadaResult<()> {
        unsafe {
            // Safety: HDU is valid for the lifetime of Self, we're checking NULL explicitly
            // Lock the writers
            if ipcbuf_lock_write(self.data_buf) != 0 {
                return Err(PsrdadaError::DadaLockingError);
            }
            if ipcbuf_lock_write(self.header_buf) != 0 {
                return Err(PsrdadaError::DadaLockingError);
            }
            // Reset
            if ipcbuf_reset(self.data_buf) != 0 {
                return Err(PsrdadaError::DadaEodError);
            }
            if ipcbuf_reset(self.header_buf) != 0 {
                return Err(PsrdadaError::DadaEodError);
            }
            // Unlock the writer
            if ipcbuf_unlock_write(self.data_buf) != 0 {
                return Err(PsrdadaError::DadaLockingError);
            }
            if ipcbuf_unlock_write(self.header_buf) != 0 {
                return Err(PsrdadaError::DadaLockingError);
            }
        }
        Ok(())
    }
}

impl Drop for DadaClient {
    fn drop(&mut self) {
        if self.allocated {
            debug!("Tearing down the data we allocated");
            unsafe {
                // Destroy data
                if ipcbuf_destroy(self.data_buf) != 0 {
                    error!("Error destroying data buffer");
                }
                // Destroy header
                if ipcbuf_destroy(self.header_buf) != 0 {
                    error!("Error destroying header buffer");
                }
            }
        }
        // Now deal with the fact that we boxed these raw ptrs
        // Just creating the box and letting them leave scope calls their destructor
        unsafe {
            Box::from_raw(self.data_buf);
            Box::from_raw(self.header_buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::DadaClientBuilder;
    use crate::tests::next_key;
    use test_log::test;

    #[test]
    fn test_connect() {
        let key = next_key();
        let _client = DadaClientBuilder::new(key).build().unwrap();
        let _connected = DadaClient::new(key).unwrap();
    }

    #[test]
    fn test_sizing() {
        let key = next_key();
        let client = DadaClientBuilder::new(key)
            .num_bufs(1)
            .buf_size(128)
            .num_headers(4)
            .header_size(64)
            .build()
            .unwrap();
        assert_eq!(client.data_buf_size(), 128);
        assert_eq!(client.data_buf_count(), 1);
        assert_eq!(client.header_buf_count(), 4);
        assert_eq!(client.header_buf_size(), 64);
    }
}
