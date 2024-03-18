//! Safe implementations of low-level reading and writing from psrdada ringbuffers.
//!
//! This module reimplements the functionality from `ipcio` from the original library.

use crate::{
    client::{DataClient, HeaderClient},
    errors::{PsrdadaError, PsrdadaResult},
};
use psrdada_sys::*;
use std::marker::PhantomData;
use tracing::{debug, error};

mod private {
    /// Private token marker to prevent library users from calling certain trait methods
    pub struct Token;
}

/// A trait for functionality shared between the header and data clients
pub trait DadaClient {
    /// Get the underlying [`psrdada_sys::ipcbuf_t`].
    /// This is a private method.
    fn buf(&mut self, _: private::Token) -> *const ipcbuf_t;

    /// Get the current buffer state. Really only useful for debugging.
    fn state(&mut self) -> State {
        unsafe { *self.buf(private::Token) }.state.into()
    }

    fn reader(&mut self) -> PsrdadaResult<Reader> {
        Reader::new(self)
    }

    fn writer(&mut self) -> PsrdadaResult<Writer> {
        Writer::new(self)
    }
}

/// The writer associated with a ringbuffer.
/// This comes into existance locked and destructs with an unlock.
pub struct Writer<'a> {
    buf: *const ipcbuf_t,
    _phantom: PhantomData<&'a ipcbuf_t>,
}

impl Writer<'_> {
    /// Lock the buffer for writing
    fn lock(&mut self) -> PsrdadaResult<()> {
        debug!("Locking buffer for writing");
        if unsafe { ipcbuf_lock_write(self.buf as *mut _) } != 0 {
            error!("Couldn't lock buffer for writing");
            Err(PsrdadaError::DadaLockingError)
        } else {
            Ok(())
        }
    }

    /// Unlock the buffer from writing
    fn unlock(&mut self) -> PsrdadaResult<()> {
        debug!("Unlocking buffer from writing");
        if unsafe { ipcbuf_unlock_write(self.buf as *mut _) } != 0 {
            error!("Couldn't unlock buffer from writing");
            Err(PsrdadaError::DadaLockingError)
        } else {
            Ok(())
        }
    }

    fn new<T: DadaClient + ?Sized>(client: &mut T) -> PsrdadaResult<Self> {
        // ipcio lines 116:130
        let mut writer = Self {
            buf: client.buf(private::Token),
            _phantom: PhantomData,
        };
        writer.lock()?;
        Ok(writer)
    }
}

impl Drop for Writer<'_> {
    fn drop(&mut self) {
        let _ = self.unlock();
    }
}

/// The reader associated with a ringbuffer
/// This comes into existance locked and destructs with an unlock.
pub struct Reader<'a> {
    buf: *const ipcbuf_t,
    _phantom: PhantomData<&'a ipcbuf_t>,
}

impl Reader<'_> {
    /// Lock the buffer for reading
    fn lock(&mut self) -> PsrdadaResult<()> {
        debug!("Locking buffer for reading");
        if unsafe { ipcbuf_lock_read(self.buf as *mut _) } != 0 {
            error!("Couldn't lock buffer for reading");
            Err(PsrdadaError::DadaLockingError)
        } else {
            Ok(())
        }
    }

    /// Unlock the buffer from reading
    fn unlock(&mut self) -> PsrdadaResult<()> {
        debug!("Unlocking buffer from reading");
        if unsafe { ipcbuf_unlock_read(self.buf as *mut _) } != 0 {
            error!("Couldn't unlock buffer from reading");
            Err(PsrdadaError::DadaLockingError)
        } else {
            Ok(())
        }
    }

    fn new<T: DadaClient + ?Sized>(client: &mut T) -> PsrdadaResult<Self> {
        let mut reader = Self {
            buf: client.buf(private::Token),
            _phantom: PhantomData,
        };
        reader.lock()?;
        Ok(reader)
    }
}

impl Drop for Reader<'_> {
    fn drop(&mut self) {
        let _ = self.unlock();
    }
}

// Implement the client functionality for both of our clients
impl DadaClient for HeaderClient<'_> {
    fn buf(&mut self, _: private::Token) -> *const ipcbuf_t {
        self.buf
    }
}
impl DadaClient for DataClient<'_> {
    fn buf(&mut self, _: private::Token) -> *const ipcbuf_t {
        self.buf
    }
}

// Include the reading and writing modules
pub mod read;
pub mod write;

#[repr(i32)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum State {
    /// Disconnected
    Disconnected = 0, // IPCBUF_DISCON
    /// Connected
    Connected = 1, //IPCBUF_VIEWER
    /// One process that writes to the buffer
    Writer = 2, // IPCBUF_WRITER
    /// Start of data flag has been raised
    Writing = 3, // IPCBUF_WRITING
    /// Next operation will change the writing state
    WriteChange = 4, // IPCBUF_WCHANGE
    /// One process that reads from the buffer
    Reader = 5, //IPCBUF_READER
    /// Start of data flag has been raised
    Reading = 6, // IPCBUF_READING
    /// End of data flag has been raised
    ReadStop = 7, // IPCBUF_RSTOP
    /// Currently viewing
    Viewing = 8, // IPCBUF_VIEWING
    /// End of data while viewing
    ViewStop = 9, // IPCBUF_VSTOP
}

impl From<i32> for State {
    fn from(value: i32) -> Self {
        match value {
            0 => State::Disconnected,
            1 => State::Connected,
            2 => State::Writer,
            3 => State::Writing,
            4 => State::WriteChange,
            5 => State::Reader,
            6 => State::Reading,
            7 => State::ReadStop,
            8 => State::Viewing,
            9 => State::ViewStop,
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        builder::DadaClientBuilder, client::HduClient, io::DadaClient, iter::DadaIterator,
        tests::next_key,
    };
    use std::io::{Read, Write};
    use test_log::test;

    #[test]
    fn test_read_write_many() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key)
            .num_bufs(4)
            .buf_size(4)
            .build()
            .unwrap();
        let (_, mut dc) = client.split();

        // Write a data to all four blocks and mark the last one as eod
        let mut writer = dc.writer().unwrap();
        for i in 0..4 {
            let mut block = writer.next().unwrap();
            block.write_all(&[0, 1, 2, 3]).unwrap();
            if i == 3 {
                block.mark_eod();
            }
        }
        drop(writer);

        // Read them back
        let mut reader = dc.reader().unwrap();
        let mut buf = [0u8; 4];
        while let Some(mut block) = reader.next() {
            block.read_exact(&mut buf).unwrap();
            assert_eq!(buf, [0, 1, 2, 3]);
        }
    }

    #[test]
    fn test_multithreaded_read_write_many() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key)
            .num_bufs(4)
            .buf_size(4)
            .build()
            .unwrap();

        // Spawn a reader thread, which will block until the data shows up
        let handle = std::thread::spawn(move || {
            let mut client = HduClient::connect(key).unwrap();
            let (_, mut dc) = client.split();
            let mut reader = dc.reader().unwrap();
            let mut buf = [0u8; 4];
            while let Some(mut block) = reader.next() {
                block.read_exact(&mut buf).unwrap();
                assert_eq!(buf, [0, 1, 2, 3]);
            }
        });

        // Write on the main thread
        let (_, mut dc) = client.split();
        let mut writer = dc.writer().unwrap();
        for i in 0..4 {
            let mut block = writer.next().unwrap();
            block.write_all(&[0, 1, 2, 3]).unwrap();
            if i == 3 {
                block.mark_eod();
            }
        }
        handle.join().unwrap();
    }

    #[test]
    fn test_read_to_vec() {}
}
