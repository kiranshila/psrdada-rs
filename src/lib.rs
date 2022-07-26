use logging::create_stderr_log;
use utils::PsrdadaError;
mod logging;
mod utils;
use crate::utils::*;
use psrdada_sys::*;

#[derive(Debug)]
pub struct DadaDBBuilder<'key> {
    key: &'key DadaKey,
    log_name: String,
    // Default things from Psrdada
    num_bufs: Option<u64>,
    buf_size: Option<u64>,
    num_headers: Option<u64>,
    header_size: Option<u64>,
}

#[derive(Debug)]
pub struct DadaDB<'key> {
    key: &'key DadaKey,
    hdu: *mut dada_hdu,
    // Buffer metadata
    num_bufs: u64,
    buf_size: u64,
    num_headers: u64,
    header_size: u64,
}

impl<'key> DadaDBBuilder<'key> {
    pub fn new(key: &'key DadaKey, log_name: &str) -> Self {
        Self {
            key,
            log_name: log_name.to_string(),
            num_bufs: None,
            buf_size: None,
            num_headers: None,
            header_size: None,
        }
    }

    pub fn num_bufs(mut self, value: u64) -> Self {
        self.num_bufs = Some(value);
        self
    }

    pub fn buf_size(mut self, value: u64) -> Self {
        self.buf_size = Some(value);
        self
    }

    pub fn num_headers(mut self, value: u64) -> Self {
        self.num_headers = Some(value);
        self
    }

    pub fn header_size(mut self, value: u64) -> Self {
        self.header_size = Some(value);
        self
    }

    /// Builder for DadaDB
    /// Buffer size will default to 4x of 128*Page Size
    /// Header size will default to 8x of Page Size
    pub fn build(self) -> PsrdadaResult<DadaDB<'key>> {
        // Unpack the things we need, defaulting as necessary
        let log_name = &self.log_name;
        let num_bufs = self.num_bufs.unwrap_or(4);
        let buf_size = self.buf_size.unwrap_or((page_size::get() as u64) * 128);
        let num_headers = self.num_headers.unwrap_or(8);
        let header_size = self.header_size.unwrap_or(page_size::get() as u64);

        // Create data block, setting readers to 1 (a la mpsc)
        let mut data = Default::default();
        unsafe {
            // Safety: Catch the error
            if ipcbuf_create(&mut data, self.key.0, num_bufs, buf_size, 1) != 0 {
                return Err(PsrdadaError::HDUInitError);
            }
        }

        // Create header block
        let mut header = Default::default();
        unsafe {
            // Safety: Catch the Error, destroy data if we fail so we don't leak memory
            if ipcbuf_create(&mut header, self.key.0 + 1, num_headers, header_size, 1) != 0 {
                // We're kinda SOL if this happens
                if ipcbuf_destroy(&mut data) != 0 {
                    return Err(PsrdadaError::HDUDestroyError);
                }
                return Err(PsrdadaError::HDUInitError);
            }
        }

        // Now we "connect" to these buffers we created
        let hdu = connect_hdu(self.key.0, &log_name).map_err(|_| {
            // Clear the memory we allocated
            unsafe {
                // Safety: header and data exist as initialzed above
                if ipcbuf_destroy(&mut data) != 0 {
                    return PsrdadaError::HDUDestroyError;
                }
                if ipcbuf_destroy(&mut header) != 0 {
                    return PsrdadaError::HDUDestroyError;
                }
            }
            PsrdadaError::HDUInitError
        })?;

        // Return built result
        Ok(DadaDB {
            key: self.key,
            hdu,
            num_bufs,
            buf_size,
            num_headers,
            header_size,
        })
    }
}

/// Sets up a `dada_hdu` by connecting to existing
/// data and header buffers and constructing the logging instance
fn connect_hdu(key: i32, log_name: &str) -> PsrdadaResult<*mut dada_hdu_t> {
    // Create the log to stderr with `log_name`
    let mut log = create_stderr_log(log_name)?;

    unsafe {
        // Safety: Log is valid, so create will return valid data
        let hdu = dada_hdu_create(&mut log);
        // Safety: hdu will be valid here due to the `dada_hdu_create`
        dada_hdu_set_key(hdu, key);
        // Safety: Catch the errors
        if dada_hdu_connect(hdu) != 0 {
            return Err(PsrdadaError::HDUInitError);
        }
        Ok(hdu)
    }
}

impl<'key> Drop for DadaDB<'key> {
    fn drop(&mut self) {
        unsafe {
            // Safety: self.hdu is a C raw ptr that is valid because we managed it during construction
            assert_eq!(
                dada_hdu_disconnect(self.hdu),
                0,
                "HDU Disconnect musn't fail"
            )
        }
    }
}

impl<'key> DadaDB<'key> {
    /// Create a DadaDB by connecting to a preexisting data + header pair
    pub fn connect(key: &'key DadaKey, log_name: &str) -> PsrdadaResult<Self> {
        let hdu = connect_hdu(key.0, log_name)?;
        unsafe {
            let header = (*hdu).header_block;
            let mut data = (*(*hdu).data_block).buf;
            Ok(Self {
                key,
                hdu,
                num_bufs: ipcbuf_get_nbufs(&mut data),
                buf_size: ipcbuf_get_bufsz(&mut data),
                num_headers: ipcbuf_get_nbufs(header),
                header_size: ipcbuf_get_bufsz(header),
            })
        }
    }

    /// Blocking next returns immutable slice of data buffer
    /// This is unsafe because we have no way to garuntee the data this returns is always valid.
    /// It is valid following this call, but for further safety garuntees, you need to memcpy
    pub fn next<const N: usize>(&self) -> PsrdadaResult<[i8; N]> {
        assert_eq!(
            N, self.buf_size as usize,
            "Container must match the buffer size"
        );
        let mut out_buf = [0i8; N];
        unsafe {
            // Lock the reader
            dada_hdu_lock_read(self.hdu);
            // Safety: HDU is valid for the lifetime of Self, we're checking NULL explicitly
            let data_ptr = ipcbuf_get_next_read(
                &mut (*(*self.hdu).data_block).buf,
                &mut (N as u64) as *mut u64,
            );
            // Check against null
            if data_ptr == 0 as *mut i8 {
                return Err(PsrdadaError::HDUReadError);
            }
            // Perform the memcpy
            let raw_slice = std::slice::from_raw_parts(data_ptr, N as usize);
            out_buf.clone_from_slice(raw_slice);
            // Unlock the reader
            dada_hdu_unlock_read(self.hdu);
        }
        Ok(out_buf)
    }

    /// Push data onto the data ring buffer
    pub fn push(&self, data: &[i8]) -> PsrdadaResult<()> {
        let len = self.buf_size;
        unsafe {
            // Safety: HDU is valid for the lifetime of Self, we're checking NULL explicitly
            // Lock the writer
            dada_hdu_lock_write(self.hdu);
            let data_ptr = ipcbuf_get_next_write(&mut (*(*self.hdu).data_block).buf);
            // Check against null
            if data_ptr == 0 as *mut i8 {
                return Err(PsrdadaError::HDUReadError);
            }
            // Perform the memcpy
            let slice = std::slice::from_raw_parts_mut(data_ptr, len as usize);
            slice.clone_from_slice(data);
            // Unlock the writer
            dada_hdu_unlock_write(self.hdu);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_construct_hdu() {
        let key = DadaKey(0xdead);
        let _my_hdu = DadaDBBuilder::new(&key, "test").build().unwrap();
    }

    #[test]
    fn test_connect_hdu() {
        let key = DadaKey(0xbeef);
        let _my_hdu = DadaDBBuilder::new(&key, "An HDU log").build().unwrap();
        let _my_connected_hdu = DadaDB::connect(&key, "Another HDU log").unwrap();
    }

    #[test]
    fn test_sizing() {
        let key = DadaKey(0x4242);
        let my_hdu = DadaDBBuilder::new(&key, "An HDU log")
            .num_bufs(1)
            .buf_size(128)
            .num_headers(4)
            .header_size(64)
            .build()
            .unwrap();
        assert_eq!(my_hdu.buf_size, 128);
        assert_eq!(my_hdu.num_bufs, 1);
        assert_eq!(my_hdu.header_size, 64);
        assert_eq!(my_hdu.num_headers, 4);
    }

    #[test]
    fn test_read_write() {
        let key = DadaKey(0x1234);
        let my_hdu = DadaDBBuilder::new(&key, "read_write")
            .buf_size(5)
            .build()
            .unwrap();
        // Push some bytes
        let bytes = [0i8, 2i8, 3i8, 4i8, 5i8];
        my_hdu.push(&bytes).unwrap();
        assert_eq!(my_hdu.next().unwrap(), bytes);
    }

    #[test]
    fn test_multithread_read_write() {
        let key = DadaKey(0x0101);
        let my_hdu = DadaDBBuilder::new(&key, "read_write")
            .buf_size(5)
            .build()
            .unwrap();
        // Push some bytes
        let bytes = [0i8, 2i8, 3i8, 4i8, 5i8];
        my_hdu.push(&bytes).unwrap();
        // Spawn the thread, but wait for the read to finish before destroying
        std::thread::scope(|s| {
            s.spawn(|| {
                let my_connected_hdu = DadaDB::connect(&key, "Another HDU log").unwrap();
                assert_eq!(my_connected_hdu.next().unwrap(), [0i8, 2i8, 3i8, 4i8, 5i8]);
            });
        });
    }
}
