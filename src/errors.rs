//! Error types for this crate

#[derive(Debug, PartialEq, Eq)]
/// All the errors we can return
pub enum PsrdadaError {
    DadaInitError,
    DadaConnectError,
    DadaDisconnectError,
    DadaDestroyError,
    DadaLockingError,
    DadaReadError,
    DadaResetError,
    DadaEodError,
    DadaSodError,
    DadaWriteError,
    DadaShmemLockError,
    UTF8Error,
    HeaderOverflow,
    HeaderParseError,
    HeaderEodError,
    GpuError,
}

pub type PsrdadaResult<T> = Result<T, PsrdadaError>;
