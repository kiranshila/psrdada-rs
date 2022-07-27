use psrdada_sys::*;

#[derive(Debug, PartialEq, Eq)]
pub enum PsrdadaError {
    HDUInitError,
    HDUConnectError,
    HDUDestroyError,
    HDULockingError,
    HDUReadError,
    HDUResetError,
    HDUEODError,
    HDUWriteError,
    HDUShmemLockError,
    MultilogError,
    FFIError,
    EODWrite,
    UTF8Error,
    HeaderOverflow,
}

pub(crate) type PsrdadaResult<T> = Result<T, PsrdadaError>;

pub(crate) fn destroy_from_key(key: i32) -> PsrdadaResult<()> {
    let mut ptr = Default::default();
    unsafe {
        ipcbuf_connect(&mut ptr, key);
        if ipcbuf_destroy(&mut ptr) != 0 {
            Err(PsrdadaError::HDUDestroyError)
        } else {
            Ok(())
        }
    }
}
