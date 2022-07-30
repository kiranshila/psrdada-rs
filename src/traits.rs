use crate::{
    logging::create_stderr_log,
    utils::{PsrdadaError, PsrdadaResult},
};
use psrdada_sys::*;
use tracing::{debug, error, info, span, warn, Level};

#[derive(Debug)]
struct HduClient {
    key: i32,
    log_name: String,
    hdu: Option<*mut dada_hdu>,
}

impl HduClient {
    #[tracing::instrument]
    /// Construct a new HduClient and try to connect
    pub fn new(key: i32, log_name: &str) -> PsrdadaResult<Self> {
        let mut client = Self {
            key,
            log_name: log_name.to_owned(),
            hdu: None,
        };
        client.connect()?;
        Ok(client)
    }

    #[tracing::instrument]
    /// Connect an existing HduClient
    fn connect(&mut self) -> PsrdadaResult<()> {
        debug!(self.key, "Connecting to dada buffer");
        // Create the log to stderr with `log_name`
        let mut log = create_stderr_log(&self.log_name)?;
        unsafe {
            let hdu = dada_hdu_create(&mut log);
            // Set the key
            dada_hdu_set_key(hdu, self.key);
            // Try to connect
            if dada_hdu_connect(hdu) != 0 {
                error!(self.key, "Could not connect to dada buffer");
                return Err(PsrdadaError::HDUInitError);
            }
            debug!("Connected!");
            self.hdu = Some(hdu);
        }
        Ok(())
    }

    #[tracing::instrument]
    /// Disconnect an existing HduClient
    fn disconnect(&mut self) -> PsrdadaResult<()> {
        match self.hdu {
            Some(hdu) => {
                debug!("Disconnecting from dada buffer");
                unsafe {
                    if dada_hdu_disconnect(hdu) != 0 {
                        error!("Could not disconnect from HDU");
                        return Err(PsrdadaError::HDUDisconnectError);
                    }
                    dada_hdu_destroy(hdu);
                }
                self.hdu = None;
            }
            None => warn!("HduClient already disconnected"),
        };
        Ok(())
    }

    #[tracing::instrument]
    /// Grab the data buffer size in bytes from a connected HduClient
    /// Returns None if not connected
    fn data_buf_size(&self) -> PsrdadaResult<Option<u64>> {
        match self.hdu {
            Some(hdu) => unsafe {
                let size = ipcbuf_get_bufsz((*hdu).data_block as *mut ipcbuf_t);
                Ok(Some(size))
            },
            None => {
                warn!("HduClient not connected");
                Ok(None)
            }
        }
    }

    #[tracing::instrument]
    /// Grab the header buffer size in bytes from a connected HduClient
    /// Returns None if not connected
    fn header_buf_size(&self) -> PsrdadaResult<Option<u64>> {
        match self.hdu {
            Some(hdu) => unsafe {
                let size = ipcbuf_get_bufsz((*hdu).header_block);
                Ok(Some(size))
            },
            None => {
                warn!("HduClient not connected");
                Ok(None)
            }
        }
    }

    #[tracing::instrument]
    /// Grab the number of data buffers in the ring from a connected HduClient
    /// Returns None if not connected
    fn data_buf_count(&self) -> PsrdadaResult<Option<u64>> {
        match self.hdu {
            Some(hdu) => unsafe {
                let size = ipcbuf_get_nbufs((*hdu).data_block as *mut ipcbuf_t);
                Ok(Some(size))
            },
            None => {
                warn!("HduClient not connected");
                Ok(None)
            }
        }
    }

    #[tracing::instrument]
    /// Grab the number of header buffers in the ring from a connected HduClient
    /// Returns None if not connected
    fn header_buf_count(&self) -> PsrdadaResult<Option<u64>> {
        match self.hdu {
            Some(hdu) => unsafe {
                let size = ipcbuf_get_nbufs((*hdu).header_block);
                Ok(Some(size))
            },
            None => {
                warn!("HduClient not connected");
                Ok(None)
            }
        }
    }

    #[tracing::instrument]
    /// Register the Hdu buffers as GPU pinned memory
    fn cuda_register(&self) -> PsrdadaResult<()> {
        match self.hdu {
            Some(hdu) => unsafe {
                if dada_cuda_dbregister(hdu) != 0 {
                    error!("Failed to register buffers as GPU pinned memory");
                    return Err(PsrdadaError::GpuError);
                }
            },
            None => warn!("HduClient not connected"),
        }
        Ok(())
    }
}
