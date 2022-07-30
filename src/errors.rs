#[derive(Debug, PartialEq, Eq)]
/// All the errors we can return
pub enum PsrdadaError {
    HDUInitError,
    HDUConnectError,
    HDUDisconnectError,
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
    GpuError,
}

pub type PsrdadaResult<T> = Result<T, PsrdadaError>;
