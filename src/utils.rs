use psrdada_sys::*;

#[derive(Debug)]
pub enum PsrdadaError {
    HDUInitError,
    HDUConnectError,
    HDUDestroyError,
    HDULockingError,
    HDUReadError,
    MultilogError,
    FFIError,
}

pub(crate) type PsrdadaResult<T> = Result<T, PsrdadaError>;

#[derive(Debug, Default)]
pub struct DadaKey(pub i32);

impl Drop for DadaKey {
    /// Consuming destructor that also destroys the underlying data and header buffers
    fn drop(&mut self) {
        // Cleanup the things we got C to malloc
        let mut data = Default::default();
        let mut header = Default::default();
        unsafe {
            // Connect to the things we're destroying
            ipcbuf_connect(&mut data, self.0);
            ipcbuf_connect(&mut header, self.0 + 1);
            // Header
            if ipcbuf_destroy(&mut header) != 0 {
                // Tracing error
            }
            // Data
            if ipcbuf_destroy(&mut data) != 0 {
                // Tracing error
            }
        }
    }
}
