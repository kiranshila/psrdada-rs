use std::collections::HashMap;

use logging::create_stderr_log;
use utils::PsrdadaError;
mod logging;
mod utils;
use crate::utils::*;
use itertools::Itertools;
use lending_iterator::prelude::*;
use psrdada_sys::*;

#[derive(Debug)]
pub struct DadaDBBuilder {
    key: i32,
    log_name: String,
    // Default things from Psrdada
    num_bufs: Option<u64>,
    buf_size: Option<u64>,
    num_headers: Option<u64>,
    header_size: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct DadaDB {
    key: i32,
    hdu: *mut dada_hdu,
    // Buffer metadata
    buf_size: u64,
    header_size: u64,
    // Track whether *we* allocated the memory
    allocated: bool,
}

#[derive(Debug)]
pub struct ReadHalf<'db> {
    parent: &'db DadaDB,
    holding_page: bool,
}

#[derive(Debug)]
pub struct WriteHalf<'db> {
    parent: &'db DadaDB,
}

impl DadaDBBuilder {
    pub fn new(key: i32, log_name: &str) -> Self {
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
    /// `lock` - locks the shared memory in RAM
    pub fn build(self, lock: bool) -> PsrdadaResult<DadaDB> {
        // Unpack the things we need, defaulting as necessary
        let key = self.key;
        let log_name = &self.log_name;
        let num_bufs = self.num_bufs.unwrap_or(4);
        let buf_size = self.buf_size.unwrap_or((page_size::get() as u64) * 128);
        let num_headers = self.num_headers.unwrap_or(8);
        let header_size = self.header_size.unwrap_or(page_size::get() as u64);

        // Create data block, setting readers to 1 (a la mpsc)
        let mut data = Default::default();
        unsafe {
            // Safety: Catch the error
            if ipcbuf_create(&mut data, self.key, num_bufs, buf_size, 1) != 0 {
                return Err(PsrdadaError::HDUInitError);
            }
        }

        // Create header block
        let mut header = Default::default();
        unsafe {
            // Safety: Catch the Error, destroy data if we fail so we don't leak memory
            if ipcbuf_create(&mut header, self.key + 1, num_headers, header_size, 1) != 0 {
                // We're kinda SOL if this happens
                if ipcbuf_destroy(&mut data) != 0 {
                    return Err(PsrdadaError::HDUDestroyError);
                }
                return Err(PsrdadaError::HDUInitError);
            }
        }

        // Lock if required, teardown everything if we fail
        if lock {
            unsafe {
                if ipcbuf_lock(&mut data) != 0 {
                    if ipcbuf_destroy(&mut data) != 0 {
                        return Err(PsrdadaError::HDUDestroyError);
                    }
                    if ipcbuf_destroy(&mut header) != 0 {
                        return Err(PsrdadaError::HDUDestroyError);
                    }
                    return Err(PsrdadaError::HDUShmemLockError);
                }

                if ipcbuf_lock(&mut header) != 0 {
                    if ipcbuf_destroy(&mut data) != 0 {
                        return Err(PsrdadaError::HDUDestroyError);
                    }
                    if ipcbuf_destroy(&mut header) != 0 {
                        return Err(PsrdadaError::HDUDestroyError);
                    }
                    return Err(PsrdadaError::HDUShmemLockError);
                }
            }
        }

        // Now we "connect" to these buffers we created
        let hdu = connect_hdu(self.key, log_name).map_err(|_| {
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
            key,
            hdu,
            buf_size,
            header_size,
            allocated: true,
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

impl Drop for DadaDB {
    fn drop(&mut self) {
        unsafe {
            // Safety: self.hdu is a C raw ptr that is valid because we managed it during construction
            assert_eq!(
                dada_hdu_disconnect(self.hdu),
                0,
                "HDU Disconnect musn't fail"
            );
            if self.allocated {
                destroy_from_key(self.key).unwrap();
                destroy_from_key(self.key + 1).unwrap();
            }
        }
    }
}

impl DadaDB {
    /// Create a DadaDB by connecting to a preexisting data + header pair
    pub fn connect(key: i32, log_name: &str) -> PsrdadaResult<Self> {
        let hdu = connect_hdu(key, log_name)?;
        unsafe {
            let header = (*hdu).header_block;
            let mut data = (*(*hdu).data_block).buf;
            Ok(Self {
                key,
                hdu,
                buf_size: ipcbuf_get_bufsz(&mut data),
                header_size: ipcbuf_get_bufsz(header),
                allocated: false,
            })
        }
    }

    /// Splits the DadaDB into the read and write pairs
    pub fn split(&self) -> (ReadHalf, WriteHalf) {
        (
            ReadHalf {
                parent: self,
                holding_page: false,
            },
            WriteHalf { parent: self },
        )
    }
}

impl<'db> WriteHalf<'db> {
    /// Push data onto the current page of the data ring buffer
    pub fn push(&mut self, data: &[u8]) -> PsrdadaResult<()> {
        // Interestingly, PSRDada doesn't complain if we try to push more data than the buffer is sized
        assert!(data.len() as u64 <= self.parent.buf_size);
        unsafe {
            // Safety: HDU is valid for the lifetime of Self, we're checking NULL explicitly
            // Lock the writer
            if dada_hdu_lock_write(self.parent.hdu) != 0 {
                return Err(PsrdadaError::HDULockingError);
            }
            let mut data_buf = (*(*self.parent.hdu).data_block).buf;
            // Read the next ptr
            let data_ptr = ipcbuf_get_next_write(&mut data_buf) as *mut u8;
            // Check against null
            if data_ptr.is_null() {
                return Err(PsrdadaError::HDUWriteError);
            }
            // Perform the memcpy
            let slice = std::slice::from_raw_parts_mut(data_ptr, data.len() as usize);
            slice.clone_from_slice(data);
            // Tell PSRDada we're done
            if ipcbuf_mark_filled(&mut data_buf, data.len() as u64) != 0 {
                return Err(PsrdadaError::HDUWriteError);
            }
            // Unlock the writer
            if dada_hdu_unlock_write(self.parent.hdu) != 0 {
                return Err(PsrdadaError::HDULockingError);
            }
        }
        Ok(())
    }

    /// Clear all the state of the writer
    pub fn reset(&mut self) -> PsrdadaResult<()> {
        unsafe {
            // Safety: HDU is valid for the lifetime of Self, we're checking NULL explicitly
            // Lock the writer
            if dada_hdu_lock_write(self.parent.hdu) != 0 {
                return Err(PsrdadaError::HDULockingError);
            }
            let mut data_buf = (*(*self.parent.hdu).data_block).buf;
            // Reset
            if ipcbuf_reset(&mut data_buf) != 0 {
                return Err(PsrdadaError::HDUEODError);
            }
            // Unlock the writer
            if dada_hdu_unlock_write(self.parent.hdu) != 0 {
                return Err(PsrdadaError::HDULockingError);
            }
        }
        Ok(())
    }
}

#[gat]
impl<'db> LendingIterator for ReadHalf<'db> {
    type Item<'next> = PsrdadaResult<&'next [u8]>;

    fn next(&'_ mut self) -> Option<Self::Item<'_>> {
        unsafe {
            // Lock the reader
            if dada_hdu_lock_read(self.parent.hdu) != 0 {
                return Some(Err(PsrdadaError::HDULockingError));
            }
            let mut data_buf = (*(*self.parent.hdu).data_block).buf;
            // If we had data already, clear the previous page
            if self.holding_page {
                if ipcbuf_mark_cleared(&mut data_buf) != 0 {
                    return Some(Err(PsrdadaError::HDUReadError));
                }
                self.holding_page = false;
            }
            // Check if we're EOD
            if ipcbuf_eod(&mut data_buf) == 1 {
                if ipcbuf_reset(&mut data_buf) != 0 {
                    return Some(Err(PsrdadaError::HDUResetError));
                }
                // Make sure we unlock before we return (I think)
                if dada_hdu_unlock_read(self.parent.hdu) != 0 {
                    return Some(Err(PsrdadaError::HDULockingError));
                }
                None
            } else {
                // If not, grab the next stuff
                let mut bytes_available = 0u64;
                // Safety: HDU is valid for the lifetime of Self, we're checking NULL explicitly
                let data_ptr =
                    ipcbuf_get_next_read(&mut data_buf, &mut bytes_available) as *const u8;
                // Check against null
                if data_ptr.is_null() {
                    return Some(Err(PsrdadaError::HDUReadError));
                }
                self.holding_page = true;
                // Construct our lifetime tracked thing
                let raw_slice = std::slice::from_raw_parts(data_ptr, bytes_available as usize);
                // Unlock the reader
                if dada_hdu_unlock_read(self.parent.hdu) != 0 {
                    return Some(Err(PsrdadaError::HDULockingError));
                }
                Some(Ok(raw_slice))
            }
        }
    }
}

// Headers

impl<'db> WriteHalf<'db> {
    /// Push a `header` map onto the ring buffer
    /// Blocking, this will wait until there is a header page available
    pub fn push_header(&self, header: &HashMap<String, String>) -> PsrdadaResult<()> {
        unsafe {
            // Lock the writer
            if dada_hdu_lock_write(self.parent.hdu) != 0 {
                return Err(PsrdadaError::HDULockingError);
            }
            let header_buf = (*self.parent.hdu).header_block;
            // Read the next ptr
            let header_ptr = ipcbuf_get_next_write(header_buf) as *mut u8;
            // Check against null
            if header_ptr.is_null() {
                return Err(PsrdadaError::HDUWriteError);
            }
            // Grab the region we'll "print" into
            let slice = std::slice::from_raw_parts_mut(
                header_ptr as *mut u8,
                self.parent.header_size as usize,
            );
            let header_str: String = Itertools::intersperse(
                header.iter().map(|(k, v)| format!("{} {}", k, v)),
                "\n".to_owned(),
            )
            .collect();
            let header_bytes = header_str.into_bytes();
            if header_bytes.len() > slice.len() {
                return Err(PsrdadaError::HeaderOverflow);
            }
            // Memcpy
            slice[0..header_bytes.len()].clone_from_slice(&header_bytes);
            // Tell PSRDada we're done
            if ipcbuf_mark_filled(header_buf, header_bytes.len() as u64) != 0 {
                return Err(PsrdadaError::HDUWriteError);
            }
            // Unlock the writer
            if dada_hdu_unlock_write(self.parent.hdu) != 0 {
                return Err(PsrdadaError::HDULockingError);
            }
        }
        Ok(())
    }
}

impl<'db> ReadHalf<'db> {
    /// Blocking read the next header from the buffer and return as a hashmap
    /// Each pair is separated by whitespace (tab or space) and pairs are separated with newlines
    pub fn next_header(&self) -> PsrdadaResult<HashMap<String, String>> {
        unsafe {
            // Lock the reader
            if dada_hdu_lock_read(self.parent.hdu) != 0 {
                return Err(PsrdadaError::HDULockingError);
            }

            let header_buf = (*self.parent.hdu).header_block;
            let mut bytes_available = 0u64;
            let header_ptr = ipcbuf_get_next_read(header_buf, &mut bytes_available) as *mut u8;
            // Check if we got nothing
            if bytes_available == 0 {
                if ipcbuf_mark_cleared(header_buf) != 0 {
                    return Err(PsrdadaError::HDUReadError);
                }
                if ipcbuf_eod(header_buf) == 1 {
                    if ipcbuf_reset(header_buf) != 0 {
                        return Err(PsrdadaError::HDUResetError);
                    }
                }
            }
            // Grab the header
            let header_str = std::str::from_utf8(std::slice::from_raw_parts(
                header_ptr as *const u8,
                bytes_available as usize,
            ))
            .map_err(|_| PsrdadaError::UTF8Error)?;
            let dict = match header_str
                .lines()
                .map(|s| {
                    let mut split = s.split_ascii_whitespace();
                    let key = split.next().to_owned();
                    let value = split.next().to_owned();
                    if let Some(key) = key {
                        if let Some(value) = value {
                            Some((key.to_owned(), value.to_owned()))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect()
            {
                Some(d) => d,
                None => return Err(PsrdadaError::UTF8Error),
            };
            // Mark as read
            if ipcbuf_mark_cleared(header_buf) != 0 {
                return Err(PsrdadaError::HDUReadError);
            }
            Ok(dict)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_construct_hdu() {
        let key = 2;
        let _my_hdu = DadaDBBuilder::new(key, "test").build(false).unwrap();
    }

    #[test]
    fn test_connect_hdu() {
        let key = 4;
        let _my_hdu = DadaDBBuilder::new(key, "An HDU log").build(false).unwrap();
        let _my_connected_hdu = DadaDB::connect(key, "Another HDU log").unwrap();
    }

    #[test]
    fn test_sizing() {
        let key = 6;
        let my_hdu = DadaDBBuilder::new(key, "An HDU log")
            .num_bufs(1)
            .buf_size(128)
            .num_headers(4)
            .header_size(64)
            .build(false)
            .unwrap();
        assert_eq!(my_hdu.buf_size, 128);
        assert_eq!(my_hdu.header_size, 64);
    }

    #[test]
    fn test_read_write() {
        let key = 8;
        let my_hdu = DadaDBBuilder::new(key, "read_write")
            .buf_size(5)
            .build(false)
            .unwrap();
        // Push some bytes
        let (mut reader, mut writer) = my_hdu.split();
        let bytes = [0u8, 2u8, 3u8, 4u8, 5u8];
        writer.push(&bytes).unwrap();
        let page = reader.next().unwrap().unwrap();
        assert_eq!(bytes, page);
    }

    #[test]
    fn test_multi_read_write() {
        let key = 10;
        let my_hdu = DadaDBBuilder::new(key, "read_write")
            .buf_size(4)
            .build(false)
            .unwrap();
        let (mut reader, mut writer) = my_hdu.split();
        // Push some bytes
        let bytes = [0u8, 2u8, 3u8, 4u8];
        writer.push(&bytes).unwrap();
        let bytes_two = [10u8, 11u8, 12u8, 13u8];
        writer.push(&bytes_two).unwrap();
        // Use an explicit scope so Rust knows the borrow is valid
        let first_page = reader.next().unwrap().unwrap();
        assert_eq!(bytes, first_page);
        let second_page = reader.next().unwrap().unwrap();
        assert_eq!(bytes_two, second_page);
    }

    #[test]
    fn test_multithread_read_write() {
        let key = 12;
        let my_hdu = DadaDBBuilder::new(key, "read_write").build(false).unwrap();
        let (_, mut writer) = my_hdu.split();
        // Push some bytes
        let bytes = [0u8, 2u8, 3u8, 4u8, 5u8];
        writer.push(&bytes).unwrap();
        // Spawn the thread, but wait for the read to finish before destroying
        std::thread::spawn(move || {
            let my_hdu = DadaDB::connect(key, "Another HDU log").unwrap();
            let mut reader = my_hdu.split().0;
            let page = reader.next().unwrap().unwrap();
            assert_eq!(bytes, page);
        })
        .join()
        .unwrap();
    }

    #[test]
    #[should_panic]
    fn test_too_much_data() {
        let key = 14;
        let my_hdu = DadaDBBuilder::new(key, "read_write")
            .buf_size(2) // Limit buffer to 2
            .build(false)
            .unwrap();
        let (mut reader, mut writer) = my_hdu.split();
        writer.push(&[0u8; 3]).unwrap();
        let page = reader.next().unwrap().unwrap();
        assert_eq!([0u8; 3], page);
    }

    #[test]
    fn test_eod_and_reset() {
        let key = 16;
        let my_hdu = DadaDBBuilder::new(key, "test").build(false).unwrap();
        // Push some bytes
        let (mut reader, mut writer) = my_hdu.split();
        // Writing 5 bytes will EOD as it's less than the buffer size
        let bytes = [0u8, 2u8, 3u8, 4u8, 5u8];
        writer.push(&bytes).unwrap();
        let page = reader.next().unwrap().unwrap();
        assert_eq!(bytes, page);
        assert_eq!(None, reader.next());
        // The None reset the buffer
        let bytes_next = [42u8, 124u8];
        writer.push(&bytes_next).unwrap();
        let page = reader.next().unwrap().unwrap();
        assert_eq!(bytes_next, page);
    }

    #[test]
    fn test_sizing_shmem() {
        let key = 18;
        let my_hdu = DadaDBBuilder::new(key, "An HDU log")
            .num_bufs(1)
            .buf_size(128)
            .num_headers(4)
            .header_size(64)
            .build(true)
            .unwrap();
        assert_eq!(my_hdu.buf_size, 128);
        assert_eq!(my_hdu.header_size, 64);
    }

    #[test]
    fn test_read_write_header() {
        let key = 20;
        let my_hdu = DadaDBBuilder::new(key, "test").build(false).unwrap();
        let header = HashMap::from([
            ("START_FREQ".to_owned(), "1530".to_owned()),
            ("STOP_FREQ".to_owned(), "1280".to_owned()),
            ("TSAMP".to_owned(), "8.193e-6".to_owned()),
        ]);
        let (reader, writer) = my_hdu.split();
        writer.push_header(&header).unwrap();
        let header_read = reader.next_header().unwrap();
        for (k, v) in header_read.into_iter() {
            assert_eq!(&header.get(&k).unwrap(), &v.as_str());
        }
    }
}
