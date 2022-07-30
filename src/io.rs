use lending_iterator::{gat, prelude::*, LendingIterator};
use psrdada_sys::*;

use crate::{client::HduClient, errors::{PsrdadaResult, PsrdadaError}};
use std::{
    mem::{self, MaybeUninit},
    ptr,
};
use tracing::{debug, error, warn};

pub struct WriteHalf<'a> {
    client: &'a HduClient,
}

pub struct ReadHalf<'a> {
    client: &'a HduClient,
    done: bool,
}

impl HduClient {
    /// Split the HduClient into the read and write halves
    pub fn split(&mut self) -> (ReadHalf, WriteHalf) {
        // Pin the CUDA memory????????
        debug!("Pinning buffers to CUDA memory");
         self.cuda_register()
             .expect("Pinning CUDA memory shouldn't fail");
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
pub struct DataBytes<'a> {
    bytes_written: usize,
    client: &'a HduClient,
    eod: bool,
    ptr: &'a mut [u8],
}

impl DataBytes<'_> {
    /// Commits the data we've written to the ringbuffer
    pub fn commit(self) {}

    /// Set the current block as the end of data
    pub fn eod(&self) {
        unsafe {
            if ipcbuf_enable_eod(self.client.data_buf() as *mut ipcbuf_t) != 0 {
                error!("Setting the end of data flag failed");
            }
        }
    }
}

impl Drop for DataBytes<'_> {
    fn drop(&mut self) {
        // Tell PSRDada how many bytes we've written
        unsafe {
            if ipcbuf_mark_filled(
                self.client.data_buf() as *mut ipcbuf_t,
                self.bytes_written as u64,
            ) != 0
            {
                error!("Error closing data block");
            }
        }
        // Unlock
        debug!("Unlocking data ringbuffer");
        unsafe {
            if dada_hdu_unlock_write(self.client.hdu) != 0 {
                error!("Error unlocking the write block");
            }
        }
    }
}

impl WriteHalf<'_> {
    /// Grabs the next available `DataBytes` that we can write to
    /// Returns None if the client fell out from under us or getting a lock errored
    pub fn next_data_bytes(&mut self) -> Option<DataBytes> {
        // Get a lock
        debug!("Locking data ringbuffer");
        unsafe {
            if dada_hdu_lock_write(self.client.hdu) != 0 {
                error!("Could not aquire a lock on the data ringbuffer");
                return None;
            }
        }
        // Grab the pointer to the next available writable memory
        debug!("Grabbing next data block");
        unsafe {
            let ptr = ipcbuf_get_next_write(self.client.data_buf() as *mut ipcbuf_t) as *mut u8;
            if ptr.is_null() {
                // AHHH
                error!("Next data block returned NULL");
                if dada_hdu_unlock_write(self.client.hdu) != 0 {
                    error!("Error unlocking the write block");
                }
                return None;
            }
            Some(DataBytes {
                bytes_written: 0,
                client: self.client,
                eod: false,
                ptr: std::slice::from_raw_parts_mut(
                    ptr,
                    self.client
                        .data_buf_size()
                        .expect("Getting buf size shouldn't fail"),
                ),
            })
        }
    }
}

impl std::io::Write for DataBytes<'_> {
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
        (&mut self.ptr[self.bytes_written..(self.bytes_written + buf.len())]).copy_from_slice(buf);
        self.bytes_written += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct DataElement<'a> {
    client: &'a HduClient,
    ptr: *const u8,
    bytes_read: usize,
    block_size: usize,
}

impl Drop for DataElement<'_> {
    fn drop(&mut self) {
        unsafe {
            // Close the block and advance the ring
            if ipcbuf_mark_cleared(self.client.data_buf() as *mut ipcbuf_t) != 0 {
                error!("Error closing the data ringbuffer for reading");
            }
            // Unlock the reader
            if dada_hdu_unlock_read(self.client.hdu) != 0 {
                error!("Error unlocking data reader")
            }
        }
    }
}

impl std::io::Read for DataElement<'_> {
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

impl DataElement<'_> {
    pub fn read_block(&mut self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.block_size) }
    }
}

#[gat]
impl LendingIterator for ReadHalf<'_> {
    type Item<'next> = DataElement<'next>;

    fn next(&'_ mut self) -> Option<Self::Item<'_>> {
        if self.done {
            return None;
        }
        // Lock the reader
        unsafe {
            if dada_hdu_lock_read(self.client.hdu) != 0 {
                error!("Error locking data reader");
                self.done = true;
                return None;
            }
        }
        // Hopefully this is an entire block
        let mut block_sz = 0u64;
        let ptr;
        unsafe {
            ptr = ipcbuf_get_next_read(self.client.data_buf() as *mut ipcbuf_t, &mut block_sz)
                as *const u8;
        }
        // Check if we're at the end
        unsafe {
            if ipcbuf_eod((*self.client.hdu).data_block as *mut ipcbuf_t) == 1 {
                self.done = true;
            }
        }
        // Make the data element
        Some(DataElement {
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

    use crate::builder::HduClientBuilder;
    use crate::tests::next_key;
    use lending_iterator::LendingIterator;
    use test_log::test;

    #[test]
    fn test_write() {
        let key = next_key();
        let mut client = HduClientBuilder::new(key, "test").build().unwrap();
        let (_, mut write) = client.split();
        let mut db = write.next_data_bytes().unwrap();
        let amnt = db
            .write(&[0u8, 1u8, 2u8, 3u8])
            .expect("Writing shouldn't fail");
        assert_eq!(amnt, 4);
    }

    #[test]
    fn test_bad_write() {
        let key = next_key();
        let mut client = HduClientBuilder::new(key, "test")
            .buf_size(2)
            .build()
            .unwrap();
        let (_, mut write) = client.split();
        let mut db = write.next_data_bytes().unwrap();
        let er = db
            .write(&[0u8, 1u8, 2u8, 3u8])
            .expect_err("Writing should fail");
    }

    #[test]
    fn test_read_write() {
        let key = next_key();
        let mut client = HduClientBuilder::new(key, "test")
            .buf_size(4)
            .build()
            .unwrap();
        let (mut read, mut write) = client.split();
        let bytes = [0u8, 1u8, 2u8, 3u8];
        // Write
        let mut db = write.next_data_bytes().unwrap();
        assert_eq!(bytes.len(), db.write(&bytes).unwrap());
        // Commit the memory to the ring buffer
        db.commit();
        // Read the bytes back
        assert_eq!(bytes, read.next().unwrap().read_block());
    }

    #[test]
    fn test_multi_read_write() {
        let key = next_key();
        let mut client = HduClientBuilder::new(key, "test")
            .buf_size(8)
            .build()
            .unwrap();
        let (mut read, mut write) = client.split();
        let bytes = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8];
        let mut db = write.next_data_bytes().unwrap();
        assert_eq!(4, db.write(&bytes[0..4]).unwrap());
        assert_eq!(4, db.write(&bytes[4..]).unwrap());
        db.commit();
        // Read the bytes back
        assert_eq!(bytes, read.next().unwrap().read_block());
    }

    #[test]
    fn test_fill_buffer_and_drain() {
        let key = next_key();
        let mut client = HduClientBuilder::new(key, "test")
            .buf_size(8)
            .build()
            .unwrap();
        let (mut read, mut write) = client.split();
        let bytes = [0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8];

        let mut db = write.next_data_bytes().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        let mut db = write.next_data_bytes().unwrap();
        assert_eq!(8, db.write(&bytes).unwrap());
        db.commit();

        // Read the bytes back
        assert_eq!(bytes, read.next().unwrap().read_block());
        assert_eq!(bytes, read.next().unwrap().read_block());
    }
}
