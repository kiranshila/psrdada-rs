use std::{io::Write, marker::PhantomData};

use psrdada_sys::*;
use tracing::{debug, error};

use super::Writer;
use crate::iter::DadaIterator;

/// The state associated with an in-progress write. This must be dropped (or [`commit`]ed) to perform more actions.
///
/// This block comes into existence with valid data and only exists as long as the data is valid.
pub struct WriteBlock<'a> {
    bytes_written: usize,
    write_all: bool,
    buf: *const ipcbuf_t,
    bytes: &'a mut [u8],
    _phantom: PhantomData<&'a ipcbuf_t>,
    eod: bool,
}

impl WriteBlock<'_> {
    /// Create a [`WriteBlock`] by mutably borrowing from the [`Writer`].
    /// This ensures we can only have one at a time.
    ///
    /// Returns an option if we successfully got a lock and a valid block.
    pub fn new(writer: &mut Writer) -> Option<Self> {
        // This follows `ipcio_open_block_write` from the c library
        // Grab the pointer to the next available writable memory
        debug!("Grabbing next writable block");
        let ptr = unsafe { ipcbuf_get_next_write(writer.buf as *mut _) } as *mut u8;
        if ptr.is_null() {
            // This really shouldn't happen
            error!("Next data block returned NULL");
            if unsafe { ipcbuf_unlock_write(writer.buf as *mut _) } != 0 {
                error!("Error unlocking the write block");
            }
            return None;
        }
        // Convert to a mutable slice
        let bufsz = unsafe { ipcbuf_get_bufsz(writer.buf as *mut _) } as usize;
        // Safety:
        // - self.ptr is valid for read of bufsz*1 by construction
        // - Data is always bytes, which are valid for all bitpatterns
        // - Total length is is not larger than isize::MAX as PSRDADA can't allocate that much
        let bytes = unsafe { std::slice::from_raw_parts_mut(ptr, bufsz) };
        Some(WriteBlock {
            bytes_written: 0,
            buf: writer.buf,
            write_all: true,
            eod: false,
            bytes,
            _phantom: PhantomData,
        })
    }

    /// Commits the data we've written to the ringbuffer
    pub fn commit(self) {}

    /// Get a mutable reference to the underlying block of bytes that we can write to.
    ///
    /// Note: You must follow this with [`increment_filled`] to tell the buffer how many bytes you
    /// have written.  However, if you don't call [`increment_filled`], we will assume you wrote the
    /// entire buffer, as that's probably the most likely usecase. Alternatively, use the `std::io::Write`
    /// trait instead. You should only really use this if you need to place certain bytes in certain
    /// places in the buffer.
    pub fn block(&mut self) -> &mut [u8] {
        self.bytes
    }

    /// Increment our internal counter of how many bytes we have written, overriding the "write all" default
    /// behavior.
    pub fn increment_filled(&mut self, n: usize) {
        self.write_all = false;
        self.bytes_written += n;
    }

    /// Tell the buffer how many bytes you have written.
    fn mark_filled(&mut self) {
        debug!("Marking current write block with number of bytes written");
        if unsafe { ipcbuf_mark_filled(self.buf as *mut _, self.bytes_written as u64) } != 0 {
            error!("Error informing the block how many bytes have been written");
        }
    }

    /// Mark this buffer as the end of data. This happens implicitly if you write fewer bytes than the size of the buffer
    pub fn mark_eod(&mut self) {
        self.eod = true;
    }
}

impl Drop for WriteBlock<'_> {
    fn drop(&mut self) {
        // Following close_block_write from ipcio
        // Set the EOD flag if appropriate
        if self.eod {
            debug!("Setting the EOD flag");
            unsafe { ipcbuf_enable_eod(self.buf as *mut _) };
        }
        if self.write_all {
            self.bytes_written = self.bytes.len();
        }
        self.mark_filled();
    }
}

// Implement the lending iterator
impl DadaIterator for Writer<'_> {
    type Item<'next> = WriteBlock<'next>
    where
        Self: 'next;

    fn next(&mut self) -> Option<Self::Item<'_>> {
        WriteBlock::new(self)
    }
}

// Implement std::io Write for the WriteBlock
impl Write for WriteBlock<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let bufsz = unsafe { ipcbuf_get_bufsz(self.buf as *mut _) } as usize;
        if self.bytes_written + buf.len() > bufsz {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Tried to write too many bytes to the buffer",
            ));
        }
        self.bytes[self.bytes_written..(self.bytes_written + buf.len())].clone_from_slice(buf);
        self.increment_filled(buf.len());
        Ok(buf.len())
    }

    // Not relevant here because the memory is unbuffered
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use test_log::test;

    use super::*;
    use crate::{builder::DadaClientBuilder, io::DadaClient, tests::next_key};

    #[test]
    fn test_write() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).build().unwrap();
        let (_, mut dc) = client.split();
        let mut writer = dc.writer().unwrap();
        // Get the writing block
        let mut block = WriteBlock::new(&mut writer).unwrap();
        // Write some data
        let bytes = block.block();
        let data = [0, 1, 2, 3];
        bytes[..4].clone_from_slice(&data);
        block.increment_filled(4);
        // And leaving scope should clean it all up
    }

    #[test]
    fn test_bad_write() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).buf_size(2).build().unwrap();
        let (_, mut dc) = client.split();
        let mut writer = dc.writer().unwrap();
        let mut db = writer.next().unwrap();
        let _er = db
            .write(&[0u8, 1u8, 2u8, 3u8])
            .expect_err("Writing should fail");
    }

    #[test]
    fn test_write_with_iter() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key)
            .num_bufs(4)
            .buf_size(4)
            .build()
            .unwrap();
        let (_, mut dc) = client.split();
        let mut writer = dc.writer().unwrap();

        // Write four times
        let mut i = 0;
        while let Some(mut block) = writer.next() {
            i += 1;
            // Write some data
            let bytes = block.block();
            let data = [0, 1, 2, 3];
            bytes[..4].clone_from_slice(&data);
            block.increment_filled(4);
            if i == 4 {
                break;
            }
        }
        // And leaving scope should clean it all up
    }

    #[test]
    fn test_write_with_std_write() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key)
            .num_bufs(4)
            .buf_size(4)
            .build()
            .unwrap();
        let (_, mut dc) = client.split();
        let mut writer = dc.writer().unwrap();

        // Write four times
        let mut i = 0;
        while let Some(mut block) = writer.next() {
            i += 1;
            let data = [0, 1, 2, 3];
            block.write_all(&data).unwrap();
            if i == 4 {
                break;
            }
        }
    }
}
