//! This module contains the safe implementation of low-level reading and writing from ringbuffers

use crate::client::DadaClient;
use lending_iterator::{gat, prelude::*, LendingIterator};
use psrdada_sys::*;
use tracing::{debug, error};

#[derive(Copy, Clone)]
pub enum BlockType {
    Data,
    Header,
}

pub struct WriteHalf<'a> {
    client: &'a DadaClient,
    ty: BlockType,
}

pub struct ReadHalf<'a> {
    client: &'a DadaClient,
    ty: BlockType,
    done: bool,
}

impl DadaClient {
    /// Split the HduClient into the read and write halves
    pub fn split(&mut self, ty: BlockType) -> (ReadHalf, WriteHalf) {
        (
            ReadHalf {
                client: self,
                done: false,
                ty,
            },
            WriteHalf { client: self, ty },
        )
    }
}

// If this struct exists, it's locked
pub struct WriteBlock<'a> {
    bytes_written: usize,
    buf: *mut ipcbuf_t,
    ptr: *mut u8,
    eod: bool,
    _marker: std::marker::PhantomData<&'a ipcbuf_t>,
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
                if ipcbuf_enable_eod(self.buf) != 0 {
                    error!("Error setting EOD");
                }
            }
        }
        // Tell PSRDada how many bytes we've written
        unsafe {
            if ipcbuf_mark_filled(self.buf, self.bytes_written as u64) != 0 {
                error!("Error closing data block");
            }
        }
        // Unlock
        debug!("Unlocking data ringbuffer");
        unsafe {
            if ipcbuf_unlock_write(self.buf) != 0 {
                error!("Error unlocking the write block");
            }
        }
    }
}

impl WriteHalf<'_> {
    /// Grabs the next available block of data we can write
    /// Returns None if the client fell out from under us or getting a lock errored
    pub fn next_write_block(&mut self) -> Option<WriteBlock> {
        let buf = match self.ty {
            BlockType::Data => self.client.data_buf,
            BlockType::Header => self.client.header_buf,
        };
        // Get a lock
        debug!("Locking ringbuffer");
        unsafe {
            if ipcbuf_lock_write(buf) != 0 {
                error!("Could not aquire a lock on the data ringbuffer");
                return None;
            }
        }
        // Grab the pointer to the next available writable memory
        debug!("Grabbing next block");
        unsafe {
            let ptr = ipcbuf_get_next_write(buf) as *mut u8;
            if ptr.is_null() {
                // AHHH
                error!("Next data block returned NULL");
                if ipcbuf_unlock_write(buf) != 0 {
                    error!("Error unlocking the write block");
                }
                return None;
            }
            Some(WriteBlock {
                bytes_written: 0,
                buf,
                eod: false,
                ptr,
                _marker: std::marker::PhantomData,
            })
        }
    }
}

impl std::io::Write for WriteBlock<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        unsafe {
            let bufsz = ipcbuf_get_bufsz(self.buf) as usize;
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

pub struct ReadBlock<'a> {
    buf: *mut ipcbuf_t,
    ptr: *const u8,
    bytes_read: usize,
    block_size: usize,
    _marker: std::marker::PhantomData<&'a ipcbuf_t>,
}

impl Drop for ReadBlock<'_> {
    fn drop(&mut self) {
        unsafe {
            // Unlock the reader
            if ipcbuf_unlock_read(self.buf) != 0 {
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
        // Grab the right ptr
        let buf = match self.ty {
            BlockType::Data => self.client.data_buf,
            BlockType::Header => self.client.header_buf,
        };
        // Lock the reader
        unsafe {
            if ipcbuf_lock_read(buf) != 0 {
                error!("Error locking data reader");
                self.done = true;
                return None;
            }
        }
        let ptr;
        let mut block_sz = 0u64;
        unsafe {
            // Hopefully this is an entire block
            ptr = ipcbuf_get_next_read(buf, &mut block_sz) as *const u8;
            if ptr.is_null() {
                error!("Next read ptr is null");
                return None;
            }
            // Mark the block as cleared - I think we can do this here (even though we haven't really cleared it yet)
            // Because we're going to unlock the read after
            if ipcbuf_mark_cleared(buf) != 0 {
                error!("Error marking data block as cleared");
            }
            // Check for EOD
            // This must happen between `mark_cleared` and `unlock_read`. No idea why.
            if ipcbuf_eod(buf) == 1 {
                self.done = true;
            }
        }
        // Make the data element
        Some(ReadBlock {
            buf,
            ptr,
            bytes_read: 0,
            block_size: block_sz as usize,
            _marker: std::marker::PhantomData,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use crate::builder::DadaClientBuilder;
    use crate::io::BlockType;
    use crate::tests::next_key;
    use lending_iterator::LendingIterator;
    use test_log::test;

    #[test]
    fn test_write() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).build().unwrap();
        let (_, mut write) = client.split(BlockType::Data);
        let mut db = write.next_write_block().unwrap();
        let amnt = db
            .write(&[0u8, 1u8, 2u8, 3u8])
            .expect("Writing shouldn't fail");
        assert_eq!(amnt, 4);
    }

    #[test]
    fn test_bad_write() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).buf_size(2).build().unwrap();
        let (_, mut write) = client.split(BlockType::Data);
        let mut db = write.next_write_block().unwrap();
        let _er = db
            .write(&[0u8, 1u8, 2u8, 3u8])
            .expect_err("Writing should fail");
    }

    #[test]
    fn test_read_write() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).buf_size(4).build().unwrap();
        let (mut read, mut write) = client.split(BlockType::Data);
        let bytes = [0u8, 1u8, 2u8, 3u8];
        // Write
        let mut db = write.next_write_block().unwrap();
        assert_eq!(bytes.len(), db.write(&bytes).unwrap());
        // Commit the memory to the ring buffer
        db.commit();
        // Read the bytes back
        assert_eq!(bytes, read.next().unwrap().read_block());
    }

    #[test]
    fn test_multi_read_write() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).buf_size(8).build().unwrap();
        let (mut read, mut write) = client.split(BlockType::Data);
        let bytes = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8];
        let mut db = write.next_write_block().unwrap();
        assert_eq!(4, db.write(&bytes[0..4]).unwrap());
        assert_eq!(4, db.write(&bytes[4..]).unwrap());
        db.commit();
        // Read the bytes back
        assert_eq!(bytes, read.next().unwrap().read_block());
    }

    #[test]
    fn test_fill_buffer_and_drain() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key)
            .buf_size(8)
            .num_bufs(4)
            .build()
            .unwrap();
        let (mut read, mut write) = client.split(BlockType::Data);

        // Fill the buffer
        let bytes = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8];
        let mut db = write.next_write_block().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        let mut db = write.next_write_block().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        let mut db = write.next_write_block().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        let mut db = write.next_write_block().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        // Drain the buffer
        assert_eq!(bytes, read.next().unwrap().read_block());
        assert_eq!(bytes, read.next().unwrap().read_block());
        assert_eq!(bytes, read.next().unwrap().read_block());
        assert_eq!(bytes, read.next().unwrap().read_block());
    }

    #[test]
    fn test_explicit_eod() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).buf_size(8).build().unwrap();
        let (mut read, mut write) = client.split(BlockType::Data);

        // Write full buffers twice, second one being eod
        let bytes = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8];
        let mut db = write.next_write_block().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        let mut db = write.next_write_block().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.eod();
        db.commit();

        // Read twice, third read should be None
        assert_eq!(bytes, read.next().unwrap().read_block());
        assert_eq!(bytes, read.next().unwrap().read_block());
        assert!(read.next().is_none());
    }

    #[test]
    fn test_implicit_eod() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).buf_size(8).build().unwrap();
        let (mut read, mut write) = client.split(BlockType::Data);

        // Write one full buffer, one less than full
        let bytes = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8];
        let mut db = write.next_write_block().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        let bytes_fewer = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8];
        let mut db = write.next_write_block().unwrap();
        assert_eq!(7, db.write(&bytes_fewer).unwrap());
        db.commit();

        // Read twice, third read should be None
        assert_eq!(bytes, read.next().unwrap().read_block());
        assert_eq!(bytes_fewer, read.next().unwrap().read_block());
        assert!(read.next().is_none());
    }
}
