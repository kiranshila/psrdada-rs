#[derive(Debug)]
pub enum PsrdadaError {
    HDUInitError,
    HDUConnectError,
    HDUDestroyError,
    MultilogError,
    FFIError,
}

pub(crate) type PsrdadaResult<T> = Result<T, PsrdadaError>;
