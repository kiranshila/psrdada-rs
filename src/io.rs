use crate::client::DadaClient;
use lending_iterator::{gat, prelude::*, LendingIterator};
use psrdada_sys::*;
use std::ptr;
use tracing::{debug, error};

pub struct WriteHalf<'a> {
    client: &'a DadaClient,
}

pub struct ReadHalf<'a> {
    client: &'a DadaClient,
    done: bool,
}

impl DadaClient {
    /// Split the HduClient into the read and write halves
    pub fn split(&mut self) -> (ReadHalf, WriteHalf) {
        (
            ReadHalf {
                client: self,
                done: false,
            },
            WriteHalf { client: self },
        )
    }
}

// If this struct exists, it's locked
pub struct DataWriteBlock<'a> {
    bytes_written: usize,
    client: &'a DadaClient,
    ptr: *mut u8,
    eod: bool,
}

impl DataWriteBlock<'_> {
    /// Commits the data we've written to the ringbuffer
    pub fn commit(self) {}

    /// Set the current block as the end of data
    /// This is implicitly set if you write fewer bytes than the size of the block
    pub fn eod(&mut self) {
        self.eod = true;
    }
}

impl Drop for DataWriteBlock<'_> {
    fn drop(&mut self) {
        // Mark this block as EOD if we need to
        if self.eod {
            // This must happen before `mark_filled` (for some reason, this is undocumented)
            unsafe {
                if ipcbuf_enable_eod(self.client.data_buf) != 0 {
                    error!("Error setting EOD");
                }
            }
        }
        // Tell PSRDada how many bytes we've written
        unsafe {
            if ipcbuf_mark_filled(self.client.data_buf, self.bytes_written as u64) != 0 {
                error!("Error closing data block");
            }
        }
        // Unlock
        debug!("Unlocking data ringbuffer");
        unsafe {
            if ipcbuf_unlock_write(self.client.data_buf) != 0 {
                error!("Error unlocking the write block");
            }
        }
    }
}

impl WriteHalf<'_> {
    /// Grabs the next available `DataBytes` that we can write to
    /// Returns None if the client fell out from under us or getting a lock errored
    pub fn next_data_write_block(&mut self) -> Option<DataWriteBlock> {
        // Get a lock
        debug!("Locking data ringbuffer");
        unsafe {
            if ipcbuf_lock_write(self.client.data_buf) != 0 {
                error!("Could not aquire a lock on the data ringbuffer");
                return None;
            }
        }
        // Grab the pointer to the next available writable memory
        debug!("Grabbing next data block");
        unsafe {
            let ptr = ipcbuf_get_next_write(self.client.data_buf) as *mut u8;
            if ptr.is_null() {
                // AHHH
                error!("Next data block returned NULL");
                if ipcbuf_unlock_write(self.client.data_buf) != 0 {
                    error!("Error unlocking the write block");
                }
                return None;
            }
            Some(DataWriteBlock {
                bytes_written: 0,
                client: self.client,
                eod: false,
                ptr,
            })
        }
    }
}

impl std::io::Write for DataWriteBlock<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.bytes_written + buf.len()
            > self.client.data_buf_size().map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "Trying to query buf size failed",
                )
            })?
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Tried to write too many bytes to the buffer",
            ));
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

pub struct DataReadBlock<'a> {
    client: &'a DadaClient,
    ptr: *const u8,
    bytes_read: usize,
    block_size: usize,
}

impl Drop for DataReadBlock<'_> {
    fn drop(&mut self) {
        unsafe {
            // Unlock the reader
            if ipcbuf_unlock_read(self.client.data_buf) != 0 {
                error!("Error unlocking data reader");
            }
        }
    }
}

impl std::io::Read for DataReadBlock<'_> {
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
            ptr::copy_nonoverlapping(src_ptr, dst_ptr, buf.len());
        }
        self.bytes_read += buf.len();
        Ok(buf.len())
    }
}

impl DataReadBlock<'_> {
    pub fn read_block(&mut self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.block_size) }
    }
}

#[gat]
impl LendingIterator for ReadHalf<'_> {
    type Item<'next> = DataReadBlock<'next>;

    fn next(&'_ mut self) -> Option<Self::Item<'_>> {
        if self.done {
            return None;
        }
        // Lock the reader
        unsafe {
            if ipcbuf_lock_read(self.client.data_buf) != 0 {
                error!("Error locking data reader");
                self.done = true;
                return None;
            }
        }
        let ptr;
        let mut block_sz = 0u64;
        unsafe {
            // Hopefully this is an entire block
            ptr = ipcbuf_get_next_read(self.client.data_buf, &mut block_sz) as *const u8;
            if ptr.is_null() {
                error!("Next read ptr is null");
                return None;
            }
            // Mark the block as cleared - I think we can do this here (even though we haven't really cleared it yet)
            // Because we're going to unlock the read after
            if ipcbuf_mark_cleared(self.client.data_buf) != 0 {
                error!("Error marking data block as cleared");
            }
            // Check for EOD
            // This must happen between `mark_cleared` and `unlock_read`. No idea why.
            if ipcbuf_eod(self.client.data_buf) == 1 {
                self.done = true;
            }
        }
        // Make the data element
        Some(DataReadBlock {
            client: self.client,
            ptr,
            bytes_read: 0,
            block_size: block_sz as usize,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use crate::builder::DadaClientBuilder;
    use crate::tests::next_key;
    use lending_iterator::LendingIterator;
    use test_log::test;

    #[test]
    fn test_write() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).build().unwrap();
        let (_, mut write) = client.split();
        let mut db = write.next_data_write_block().unwrap();
        let amnt = db
            .write(&[0u8, 1u8, 2u8, 3u8])
            .expect("Writing shouldn't fail");
        assert_eq!(amnt, 4);
    }

    #[test]
    fn test_bad_write() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).buf_size(2).build().unwrap();
        let (_, mut write) = client.split();
        let mut db = write.next_data_write_block().unwrap();
        let er = db
            .write(&[0u8, 1u8, 2u8, 3u8])
            .expect_err("Writing should fail");
    }

    #[test]
    fn test_read_write() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).buf_size(4).build().unwrap();
        let (mut read, mut write) = client.split();
        let bytes = [0u8, 1u8, 2u8, 3u8];
        // Write
        let mut db = write.next_data_write_block().unwrap();
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
        let (mut read, mut write) = client.split();
        let bytes = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8];
        let mut db = write.next_data_write_block().unwrap();
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
        let (mut read, mut write) = client.split();

        // Fill the buffer
        let bytes = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8];
        let mut db = write.next_data_write_block().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        let mut db = write.next_data_write_block().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        let mut db = write.next_data_write_block().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        let mut db = write.next_data_write_block().unwrap();
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
        let (mut read, mut write) = client.split();

        // Write full buffers twice, second one being eod
        let bytes = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8];
        let mut db = write.next_data_write_block().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        let mut db = write.next_data_write_block().unwrap();
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
        let (mut read, mut write) = client.split();

        // Write one full buffer, one less than full
        let bytes = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8];
        let mut db = write.next_data_write_block().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        let bytes_fewer = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8];
        let mut db = write.next_data_write_block().unwrap();
        assert_eq!(7, db.write(&bytes_fewer).unwrap());
        db.commit();

        // Read twice, third read should be None
        assert_eq!(bytes, read.next().unwrap().read_block());
        assert_eq!(bytes_fewer, read.next().unwrap().read_block());
        assert!(read.next().is_none());
    }
}
