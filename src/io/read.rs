use std::marker::PhantomData;

use psrdada_sys::*;
use tracing::{debug, error};

use super::Reader;
use crate::iter::DadaIterator;

/// The state associated with an in-progress read. This must be dropped to perform more actions or consumed with [`done`].
///
/// This block comes into with valid data and only exists as long as it is valid
pub struct ReadBlock<'a> {
    buf: *const ipcbuf_t,
    bytes_read: usize,
    bytes: &'a [u8],
    _phantom: PhantomData<&'a ipcbuf_t>,
}

impl ReadBlock<'_> {
    /// Create a [`ReadBlock`] by mutably borrowing from the [`Reader`].
    /// This ensures we can only have one at a time.
    ///
    /// Returns an option if we successfully got a valid block.
    pub fn new(reader: &mut Reader) -> Option<Self> {
        // Test for EOD
        if unsafe { ipcbuf_eod(reader.buf as *mut _) } == 1 {
            debug!("EOD set - returning None");
            return None;
        }
        // Following `ipcio` lines 493 onwards
        // Grab the pointer to the next available readable memory
        debug!("Grabbing next readable block");
        let mut block_size = 0;
        let ptr =
            unsafe { ipcbuf_get_next_read(reader.buf as *mut _, &mut block_size) } as *const u8;
        let bytes = unsafe { std::slice::from_raw_parts(ptr, block_size as usize) };
        if ptr.is_null() {
            // This really shouldn't happen
            error!("Next block returned NULL");
            // Unlock? I guess?
            if unsafe { ipcbuf_unlock_read(reader.buf as *mut _) } != 0 {
                error!("Error unlocking the read block");
            }
            return None;
        }
        Some(Self {
            buf: reader.buf,
            bytes_read: 0,
            bytes,
            _phantom: PhantomData,
        })
    }

    /// Consumes the block, marking it as fully read.
    pub fn done(self) {}

    /// Get the underlying block of bytes for this block.
    pub fn block(&mut self) -> &[u8] {
        self.bytes
    }
}

impl Drop for ReadBlock<'_> {
    fn drop(&mut self) {
        // Following `close_block_read` from lines 541 onwards
        if unsafe { ipcbuf_mark_cleared(self.buf as *mut _) } != 0 {
            error!("Couldn't mark the block as fully read");
        }
    }
}

// Implement our lending iterator for the read blocks
impl DadaIterator for Reader<'_> {
    type Item<'next> = ReadBlock<'next>
    where
        Self: 'next;

    fn next(&mut self) -> Option<Self::Item<'_>> {
        ReadBlock::new(self)
    }
}

//Implement std::io::Read for the ReadBlock
impl std::io::Read for ReadBlock<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // Read as many bytes as we can into buf
        // This has a few different forms:
        // 1. self.bytes.len() < buf.len()
        //    -> reads self.bytes.len() bytes
        // 2. self.bytes.len() = buf.len()
        //    -> reads self.bytes.len() bytes
        // 3. self.bytes.len() > buf.len()
        //    -> reads buf.len() bytes

        //  But this is stateful, as we move around a read ptr,
        // so we need to account for that too
        let bytes_left_to_read = self.block().len() - self.bytes_read;
        if bytes_left_to_read == 0 {
            // Nothing to read, EOF
            Ok(0)
        } else if bytes_left_to_read <= buf.len() {
            buf[..bytes_left_to_read].clone_from_slice(&self.bytes[self.bytes_read..]);
            self.bytes_read += bytes_left_to_read;
            Ok(bytes_left_to_read)
        } else {
            // bytes_left_to_read > buf.len()
            let bytes_to_read = buf.len();
            buf.clone_from_slice(&self.bytes[self.bytes_read..(self.bytes_read + bytes_to_read)]);
            self.bytes_read += bytes_to_read;
            Ok(bytes_to_read)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use test_log::test;

    use crate::{
        builder::DadaClientBuilder,
        io::{read::ReadBlock, DadaClient},
        iter::DadaIterator,
        tests::next_key,
    };

    #[test]
    fn test_read_write() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).build().unwrap();
        let (_, mut dc) = client.split();

        // Write some data
        let mut writer = dc.writer().unwrap();
        let mut block = writer.next().unwrap();
        block.write_all(&[0, 1, 2, 3]).unwrap();
        block.commit();
        drop(writer);

        // Read it back
        let mut reader = dc.reader().unwrap();
        let mut block = ReadBlock::new(&mut reader).unwrap();
        assert_eq!(block.block().len(), 4);
        assert_eq!(block.block(), &[0, 1, 2, 3]);
        block.done();
    }

    #[test]
    fn test_read_write_implicit_eod() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).buf_size(4).build().unwrap();
        let (_, mut dc) = client.split();

        // Write some data
        let mut writer = dc.writer().unwrap();
        let mut block = writer.next().unwrap();
        block.write_all(&[0, 1, 2]).unwrap();
        block.commit();
        drop(writer);

        // Read it back
        let mut reader = dc.reader().unwrap();
        let mut block = ReadBlock::new(&mut reader).unwrap();
        assert_eq!(block.block().len(), 3);
        assert_eq!(block.block(), &[0, 1, 2]);
        block.done();

        // This one is eod now
        let block = ReadBlock::new(&mut reader);
        assert!(block.is_none())
    }

    #[test]
    fn test_read_write_explicit_eod() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).buf_size(4).build().unwrap();
        let (_, mut dc) = client.split();

        // Write some data
        let mut writer = dc.writer().unwrap();
        let mut block = writer.next().unwrap();
        block.write_all(&[0, 1, 2, 3]).unwrap();
        block.mark_eod();
        block.commit();
        drop(writer);

        // Read it back
        let mut reader = dc.reader().unwrap();
        let mut block = ReadBlock::new(&mut reader).unwrap();
        assert_eq!(block.block().len(), 4);
        assert_eq!(block.block(), &[0, 1, 2, 3]);
        block.done();

        // This one is eod now
        let block = ReadBlock::new(&mut reader);
        assert!(block.is_none())
    }

    #[test]
    fn test_read_write_with_iter() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).build().unwrap();
        let (_, mut dc) = client.split();

        // Write some data
        let mut writer = dc.writer().unwrap();
        let mut block = writer.next().unwrap();
        block.write_all(&[0, 1, 2, 3]).unwrap();
        block.commit();
        drop(writer);

        // Read it back
        let mut reader = dc.reader().unwrap();
        let mut block = reader.next().unwrap();
        assert_eq!(block.block().len(), 4);
        assert_eq!(block.block(), &[0, 1, 2, 3]);
        block.done();
    }

    #[test]
    fn test_read_write_with_std_read() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).build().unwrap();
        let (_, mut dc) = client.split();

        // Write some data
        let mut writer = dc.writer().unwrap();
        let mut block = writer.next().unwrap();
        block.write_all(&[0, 1, 2, 3]).unwrap();
        block.commit();
        drop(writer);

        // Read it back
        let mut reader = dc.reader().unwrap();
        let mut block = reader.next().unwrap();
        let mut buf = [0u8; 4];
        block.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [0, 1, 2, 3]);
        block.done();
    }

    #[test]
    fn test_read_write_with_std_read_to_vec() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).build().unwrap();
        let (_, mut dc) = client.split();

        // Write some data
        let mut writer = dc.writer().unwrap();
        let mut block = writer.next().unwrap();
        block.write_all(&[0, 1, 2, 3]).unwrap();
        block.commit();
        drop(writer);

        // Read it back
        let mut reader = dc.reader().unwrap();
        let mut block = ReadBlock::new(&mut reader).unwrap();
        let mut buf = vec![];
        block.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, [0, 1, 2, 3]);
        block.done();
    }
}
