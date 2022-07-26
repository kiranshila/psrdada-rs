use std::ffi::CString;

use crate::utils::{PsrdadaError, PsrdadaResult};
use psrdada_sys::{
    multilog_add, multilog_open, multilog_t, FILE, LOG_ALERT, LOG_CRIT, LOG_DEBUG, LOG_EMERG,
    LOG_ERR, LOG_INFO, LOG_NOTICE, LOG_WARNING,
};

// Sketchy danger
const STDERR_FILENO: i32 = 2;

#[derive(Debug)]
#[repr(u32)]
pub(crate) enum MultilogLevels {
    Emergency = LOG_EMERG,
    Alert = LOG_ALERT,
    Critical = LOG_CRIT,
    Error = LOG_ERR,
    Warning = LOG_WARNING,
    Notice = LOG_NOTICE,
    Info = LOG_INFO,
    Debug = LOG_DEBUG,
}

pub(crate) fn create_stderr_log(name: &str) -> PsrdadaResult<multilog_t> {
    let name_cstr = CString::new(name).map_err(|_| PsrdadaError::MultilogError)?;
    unsafe {
        // Safety: The FD we give here should be valid (it's STDERR)
        // and if multilog_open didn't fail, ptr::read will be valid
        let log_ptr = multilog_open(name_cstr.as_ptr(), 0);
        if multilog_add(log_ptr, STDERR_FILENO as *mut FILE) != 0 {
            Err(PsrdadaError::MultilogError)
        } else {
            Ok(std::ptr::read(log_ptr))
        }
    }
}

// TODO: We should capture all the log output and incorporate into non FD-based logging
// i.e. tracing etc.
