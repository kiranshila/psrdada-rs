//! Implementations for the paired and split clients

use std::marker::PhantomData;

use psrdada_sys::*;
use tracing::{debug, error, warn};

use crate::errors::{PsrdadaError, PsrdadaResult};

#[derive(Debug)]
/// The struct that stores the Header + Data ringbuffers
pub struct DadaClient {
    allocated: bool,
    pub(crate) data_buf: *const ipcbuf_t,
    pub(crate) header_buf: *const ipcbuf_t,
}

/// Client for working with the header ringbuffer
pub struct HeaderClient<'a> {
    pub(crate) buf: *const ipcbuf_t,
    _phantom: PhantomData<&'a ipcbuf_t>,
}

/// Client for working with the data ringbuffer
pub struct DataClient<'a> {
    pub(crate) buf: *const ipcbuf_t,
    _phantom: PhantomData<&'a ipcbuf_t>,
}

// Splitting borrows
impl DadaClient {
    /// Split the DadaClient into header and data clients
    pub fn split(&mut self) -> (HeaderClient, DataClient) {
        (
            HeaderClient {
                buf: self.header_buf,
                _phantom: PhantomData,
            },
            DataClient {
                buf: self.data_buf,
                _phantom: PhantomData,
            },
        )
    }
}

impl DadaClient {
    #[tracing::instrument]
    /// Internal method used by builder (we know we allocated it)
    /// # Safety
    /// The pointers passed to this build function must *not* be shared with anything else.
    /// Additionally, these pointers *must* come from Box::into_raw
    pub(crate) unsafe fn build(
        data_buf: *mut ipcbuf_t,
        header_buf: *mut ipcbuf_t,
    ) -> PsrdadaResult<Self> {
        let mut s = Self {
            data_buf: data_buf as *const _,
            header_buf: header_buf as *const _,
            allocated: true,
        };
        // Clear our state, just to make sure
        s.reset()?;
        Ok(s)
    }

    #[tracing::instrument]
    /// Construct a new DadaClient and connect to existing ring buffers
    pub fn new(key: i32) -> PsrdadaResult<Self> {
        let (data_buf, header_buf) = Self::connect(key)?;
        let s = Self {
            data_buf,
            header_buf,
            allocated: false,
        };
        Ok(s)
    }

    #[tracing::instrument]
    /// Internal method to actually build and connect
    fn connect(key: i32) -> PsrdadaResult<(*const ipcbuf_t, *const ipcbuf_t)> {
        debug!(key, "Connecting to dada buffer");
        unsafe {
            let data_buf = Box::into_raw(Box::default());
            if ipcbuf_connect(data_buf, key) != 0 {
                error!(key, "Could not connect to data buffer");
                return Err(PsrdadaError::DadaInitError);
            }
            let header_buf = Box::into_raw(Box::default());
            if ipcbuf_connect(header_buf, key + 1) != 0 {
                error!(key, "Could not connect to header buffer");
                return Err(PsrdadaError::DadaInitError);
            }
            debug!("Connected!");
            Ok((data_buf as *const _, header_buf as *const _))
        }
    }

    #[tracing::instrument]
    /// Disconnect an existing DadaClient
    fn disconnect(&mut self) -> PsrdadaResult<()> {
        debug!("Disconnecting from dada buffer");
        unsafe {
            if ipcbuf_disconnect(self.data_buf as *mut _) != 0 {
                error!("Could not disconnect from data buffer");
                return Err(PsrdadaError::DadaDisconnectError);
            }
            if ipcbuf_disconnect(self.header_buf as *mut _) != 0 {
                error!("Could not disconnect from header buffer");
                return Err(PsrdadaError::DadaDisconnectError);
            }
        }
        Ok(())
    }

    #[tracing::instrument]
    /// Grab the data buffer size in bytes from a connected DadaClient
    pub fn data_buf_size(&self) -> usize {
        unsafe { ipcbuf_get_bufsz(self.data_buf as *mut _) as usize }
    }

    #[tracing::instrument]
    /// Grab the header buffer size in bytes from a connected DadaClient
    pub fn header_buf_size(&self) -> usize {
        unsafe { ipcbuf_get_bufsz(self.header_buf as *mut _) as usize }
    }

    #[tracing::instrument]
    /// Grab the number of data buffers in the ring from a connected DadaClient
    pub fn data_buf_count(&self) -> usize {
        unsafe { ipcbuf_get_nbufs(self.data_buf as *mut _) as usize }
    }

    #[tracing::instrument]
    /// Grab the number of header buffers in the ring from a connected DadaClient
    pub fn header_buf_count(&self) -> usize {
        unsafe { ipcbuf_get_nbufs(self.header_buf as *mut _) as usize }
    }

    #[tracing::instrument]
    /// Reset the state of everything
    pub fn reset(&mut self) -> PsrdadaResult<()> {
        unsafe {
            // Safety: HDU is valid for the lifetime of Self, we're checking NULL explicitly
            // Lock the writers
            if ipcbuf_lock_write(self.data_buf as *mut _) != 0 {
                return Err(PsrdadaError::DadaLockingError);
            }
            if ipcbuf_lock_write(self.header_buf as *mut _) != 0 {
                return Err(PsrdadaError::DadaLockingError);
            }
            // Reset
            if ipcbuf_reset(self.data_buf as *mut _) != 0 {
                return Err(PsrdadaError::DadaEodError);
            }
            if ipcbuf_reset(self.header_buf as *mut _) != 0 {
                return Err(PsrdadaError::DadaEodError);
            }
            // Unlock the writer
            if ipcbuf_unlock_write(self.data_buf as *mut _) != 0 {
                return Err(PsrdadaError::DadaLockingError);
            }
            if ipcbuf_unlock_write(self.header_buf as *mut _) != 0 {
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
                if ipcbuf_destroy(self.data_buf as *mut _) != 0 {
                    error!("Error destroying data buffer");
                }
                // Destroy header
                if ipcbuf_destroy(self.header_buf as *mut _) != 0 {
                    error!("Error destroying header buffer");
                }
            }
        }
        // Now deal with the fact that we boxed these raw ptrs
        // Safety: data_buf and header_buf are boxed
        unsafe {
            drop(Box::from_raw(self.data_buf as *mut ipcbuf_t));
            drop(Box::from_raw(self.header_buf as *mut ipcbuf_t));
        }
    }
}

#[cfg(test)]
mod tests {
    use test_log::test;

    use super::*;
    use crate::{builder::DadaClientBuilder, tests::next_key};

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

    #[test]
    #[ignore] // This fails in CI because of the virtualization env
    fn test_build_lock_page() {
        let key = next_key();
        let _client = DadaClientBuilder::new(key)
            .lock(true)
            .page(true)
            .build()
            .unwrap();
    }
}
