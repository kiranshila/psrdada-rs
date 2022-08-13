//! Safe implementations of low-level reading and writing from psrdada ringbuffers

use lending_iterator::{gat, prelude::*, LendingIterator};
use psrdada_sys::*;
use tracing::{debug, error};

use crate::client::{DataClient, HeaderClient};

/// The writer associated with a ringbuffer
pub struct WriteHalf<'a> {
    buf: &'a *mut ipcbuf_t,
}

/// The reader associated with a ringbuffer
pub struct ReadHalf<'a> {
    buf: &'a *mut ipcbuf_t,
    done: bool,
}

impl DataClient<'_> {
    /// Get a reader for this DataClient. This is mutually exclusive with `writer`
    pub fn reader(&mut self) -> ReadHalf {
        ReadHalf {
            buf: self.buf,
            done: false,
        }
    }

    /// Get a writer for this DataClient. This is mutually exclusive with `reader`
    pub fn writer(&mut self) -> WriteHalf {
        WriteHalf { buf: self.buf }
    }
}

impl HeaderClient<'_> {
    /// Get a reader for this HeaderClient. This is mutually exclusive with `writer`
    pub fn reader(&mut self) -> ReadHalf {
        ReadHalf {
            buf: self.buf,
            done: false,
        }
    }

    /// Get a writer for this HeaderClient. This is mutually exclusive with `reader`
    pub fn writer(&mut self) -> WriteHalf {
        WriteHalf { buf: self.buf }
    }
}

// If this struct exists, it's locked
/// The state associated with an in-progress write. This must be dropped (or `commit()`ed) to perform more actions.
pub struct WriteBlock<'a> {
    bytes_written: usize,
    buf: &'a *mut ipcbuf_t,
    ptr: *mut u8,
    eod: bool,
}

impl WriteBlock<'_> {
    /// Commits the data we've written to the ringbuffer
    pub fn commit(self) {}

    /// Set the current block as the end of data
    /// This is implicitly set if you write fewer bytes than the size of the block
    pub fn eod(&mut self) {
        self.eod = true;
    }
}

impl Drop for WriteBlock<'_> {
    fn drop(&mut self) {
        // Mark this block as EOD if we need to
        if self.eod {
            // This must happen before `mark_filled` (for some reason, this is undocumented)
            unsafe {
                if ipcbuf_enable_eod(*self.buf) != 0 {
                    error!("Error setting EOD");
                }
            }
        }
        // Tell PSRDada how many bytes we've written
        unsafe {
            if ipcbuf_mark_filled(*self.buf, self.bytes_written as u64) != 0 {
                error!("Error closing data block");
            }
        }
        // Unlock
        debug!("Unlocking ringbuffer");
        unsafe {
            if ipcbuf_unlock_write(*self.buf) != 0 {
                error!("Error unlocking the write block");
            }
        }
    }
}

#[gat]
impl LendingIterator for WriteHalf<'_> {
    type Item<'next> = WriteBlock<'next>;

    fn next(&'_ mut self) -> Option<Self::Item<'_>> {
        // Get a lock
        debug!("Locking ringbuffer");
        unsafe {
            if ipcbuf_lock_write(*self.buf) != 0 {
                error!("Could not aquire a lock on the data ringbuffer");
                return None;
            }
        }
        // Grab the pointer to the next available writable memory
        debug!("Grabbing next block");
        unsafe {
            let ptr = ipcbuf_get_next_write(*self.buf) as *mut u8;
            if ptr.is_null() {
                // AHHH
                error!("Next data block returned NULL");
                if ipcbuf_unlock_write(*self.buf) != 0 {
                    error!("Error unlocking the write block");
                }
                return None;
            }
            Some(WriteBlock {
                bytes_written: 0,
                buf: self.buf,
                eod: false,
                ptr,
            })
        }
    }
}

impl std::io::Write for WriteBlock<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        unsafe {
            let bufsz = ipcbuf_get_bufsz(*self.buf) as usize;
            if self.bytes_written + buf.len() > bufsz {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Tried to write too many bytes to the buffer",
                ));
            }
        }
        // memcpy from the buf to the ptr
        unsafe {
            let dst_ptr = self.ptr.add(self.bytes_written);
            let src_ptr = buf.as_ptr();
            std::ptr::copy_nonoverlapping(src_ptr, dst_ptr, buf.len());
        }
        self.bytes_written += buf.len();
        Ok(buf.len())
    }

    // Not relevant here
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The state associated with an in-progress read. This must be dropped to perform more actions.
pub struct ReadBlock<'a> {
    buf: &'a *mut ipcbuf_t,
    ptr: *const u8,
    bytes_read: usize,
    block_size: usize,
}

impl Drop for ReadBlock<'_> {
    fn drop(&mut self) {
        unsafe {
            // Unlock the reader
            if ipcbuf_unlock_read(*self.buf) != 0 {
                error!("Error unlocking block reader");
            }
        }
    }
}

impl std::io::Read for ReadBlock<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.bytes_read + buf.len() > self.block_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Tried to read too many bytes from the buffer",
            ));
        }
        // memcpy from the buf to the ptr
        unsafe {
            let src_ptr = self.ptr.add(self.bytes_read);
            let dst_ptr = buf.as_mut_ptr();
            std::ptr::copy_nonoverlapping(src_ptr, dst_ptr, buf.len());
        }
        self.bytes_read += buf.len();
        Ok(buf.len())
    }
}

impl ReadBlock<'_> {
    /// Read an entire block from `ReadBlock`
    pub fn read_block(&mut self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.block_size) }
    }
}

#[gat]
impl LendingIterator for ReadHalf<'_> {
    type Item<'next> = ReadBlock<'next>;

    fn next(&'_ mut self) -> Option<Self::Item<'_>> {
        if self.done {
            return None;
        }
        // Lock the reader
        unsafe {
            if ipcbuf_lock_read(*self.buf) != 0 {
                error!("Error locking data reader");
                self.done = true;
                return None;
            }
        }
        let ptr;
        let mut block_sz = 0u64;
        unsafe {
            // Hopefully this is an entire block
            ptr = ipcbuf_get_next_read(*self.buf, &mut block_sz) as *const u8;
            if ptr.is_null() {
                error!("Next read ptr is null");
                return None;
            }
            // Mark the block as cleared - I think we can do this here (even though we haven't really cleared it yet)
            // Because we're going to unlock the read after
            if ipcbuf_mark_cleared(*self.buf) != 0 {
                error!("Error marking data block as cleared");
            }
            // Check for EOD
            // This must happen between `mark_cleared` and `unlock_read`. No idea why.
            if ipcbuf_eod(*self.buf) == 1 {
                self.done = true;
            }
        }
        // Make the data element
        Some(ReadBlock {
            buf: self.buf,
            ptr,
            bytes_read: 0,
            block_size: block_sz as usize,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use lending_iterator::LendingIterator;
    use test_log::test;

    use crate::{builder::DadaClientBuilder, client::DadaClient, tests::next_key};

    #[test]
    fn test_write() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).build().unwrap();
        let (_, mut dc) = client.split();
        let mut writer = dc.writer();
        let mut db = writer.next().unwrap();
        let amnt = db
            .write(&[0u8, 1u8, 2u8, 3u8])
            .expect("Writing shouldn't fail");
        assert_eq!(amnt, 4);
    }

    #[test]
    fn test_bad_write() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).buf_size(2).build().unwrap();
        let (_, mut dc) = client.split();
        let mut writer = dc.writer();
        let mut db = writer.next().unwrap();
        let _er = db
            .write(&[0u8, 1u8, 2u8, 3u8])
            .expect_err("Writing should fail");
    }

    #[test]
    fn test_read_write() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).buf_size(4).build().unwrap();
        let (_, mut dc) = client.split();
        let bytes = [0u8, 1u8, 2u8, 3u8];
        // Write
        let mut writer = dc.writer();
        let mut db = writer.next().unwrap();
        assert_eq!(bytes.len(), db.write(&bytes).unwrap());
        // Commit the memory to the ring buffer
        db.commit();
        // Read the bytes back
        let mut reader = dc.reader();
        assert_eq!(bytes, reader.next().unwrap().read_block());
    }

    #[test]
    fn test_multithreaded_read_write() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).buf_size(4).build().unwrap();

        // Spawn a reader thread, which will block until the data shows up
        let handle = std::thread::spawn(move || {
            let mut client = DadaClient::new(key).unwrap();
            let (_, mut dc) = client.split();
            let mut reader = dc.reader();
            assert_eq!([0u8, 1u8, 2u8, 3u8], reader.next().unwrap().read_block());
        });

        // Write on the main thread
        let (_, mut dc) = client.split();
        let bytes = [0u8, 1u8, 2u8, 3u8];
        // Write
        let mut writer = dc.writer();
        let mut db = writer.next().unwrap();
        assert_eq!(bytes.len(), db.write(&bytes).unwrap());
        // Commit the memory to the ring buffer
        db.commit();

        // Join the thread
        handle.join().unwrap()
    }

    #[test]
    fn test_multi_read_write() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).buf_size(8).build().unwrap();
        let (_, mut dc) = client.split();
        let bytes = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8];
        let mut writer = dc.writer();

        let mut db = writer.next().unwrap();
        assert_eq!(4, db.write(&bytes[0..4]).unwrap());
        assert_eq!(4, db.write(&bytes[4..]).unwrap());
        db.commit();
        // Read the bytes back
        let mut reader = dc.reader();
        assert_eq!(bytes, reader.next().unwrap().read_block());
    }

    #[test]
    fn test_fill_buffer_and_drain() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key)
            .buf_size(8)
            .num_bufs(4)
            .build()
            .unwrap();
        let (_, mut dc) = client.split();

        let mut writer = dc.writer();

        // Fill the buffer
        let bytes = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8];
        let mut db = writer.next().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        let mut db = writer.next().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        let mut db = writer.next().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        let mut db = writer.next().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        // Drain the buffer
        let mut reader = dc.reader();
        assert_eq!(bytes, reader.next().unwrap().read_block());
        assert_eq!(bytes, reader.next().unwrap().read_block());
        assert_eq!(bytes, reader.next().unwrap().read_block());
        assert_eq!(bytes, reader.next().unwrap().read_block());
    }

    #[test]
    fn test_explicit_eod() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).buf_size(8).build().unwrap();
        let (_, mut dc) = client.split();

        let mut writer = dc.writer();

        // Write full buffers twice, second one being eod
        let bytes = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8];
        let mut db = writer.next().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        let mut db = writer.next().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.eod();
        db.commit();

        let mut reader = dc.reader();

        // Read twice, third read should be None
        assert_eq!(bytes, reader.next().unwrap().read_block());
        assert_eq!(bytes, reader.next().unwrap().read_block());
        assert!(reader.next().is_none());
    }

    #[test]
    fn test_implicit_eod() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).buf_size(8).build().unwrap();
        let (_, mut dc) = client.split();

        let mut writer = dc.writer();

        // Write one full buffer, one less than full
        let bytes = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8];
        let mut db = writer.next().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        let bytes_fewer = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8];
        let mut db = writer.next().unwrap();
        assert_eq!(7, db.write(&bytes_fewer).unwrap());
        db.commit();

        let mut reader = dc.reader();

        // Read twice, third read should be None
        assert_eq!(bytes, reader.next().unwrap().read_block());
        assert_eq!(bytes_fewer, reader.next().unwrap().read_block());
        assert!(reader.next().is_none());
    }

    #[test]
    fn test_headers() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).build().unwrap();
        let (mut hc, _) = client.split();

        let mut writer = hc.writer();

        let bytes = [0u8; 128];
        let mut hb = writer.next().unwrap();
        assert_eq!(128, hb.write(&bytes).unwrap());
        hb.commit();

        let mut reader = hc.reader();
        assert_eq!(bytes, reader.next().unwrap().read_block());
    }
}
