//! Higher level abstractions for working with the Read and Write halves as well as directly pushing and poping from the data ringbuffer

use std::io::Write;

use lending_iterator::LendingIterator;

use crate::{
    client::DadaClient,
    errors::{PsrdadaError, PsrdadaResult},
    io::{ReadHalf, WriteHalf},
};

impl WriteHalf<'_> {
    /// Push data onto the corresponding ringbuffer and return how many bytes we wrote
    pub fn push(&mut self, data: &[u8]) -> PsrdadaResult<usize> {
        let mut block = match self.next_write_block() {
            Some(b) => b,
            None => return Err(PsrdadaError::DadaWriteError),
        };
        block.write(data).map_err(|_| PsrdadaError::DadaWriteError)
    }
}

impl ReadHalf<'_> {
    /// Pop the next full block off the ringbuffer, return as an owned Vec of bytes.
    /// Returns None if we hit end of data
    pub fn pop(&mut self) -> Option<Vec<u8>> {
        let mut block = match self.next() {
            Some(b) => b,
            None => return None,
        };
        Some(block.read_block().to_vec())
    }
}

impl DadaClient {
    /// Push data onto the data ringbuffer and return how many bytes we wrote
    pub fn push_data(&mut self, data: &[u8]) -> PsrdadaResult<usize> {
        let (_, mut dc) = self.split();
        let mut writer = dc.writer();
        writer.push(data)
    }
    /// Pop the next full block of data off the data ringbuffer, return as an owned Vec of bytes.
    /// Returns None if we hit end of data
    pub fn pop_data(&mut self) -> Option<Vec<u8>> {
        let (_, mut dc) = self.split();
        let mut reader = dc.reader();
        reader.pop()
    }
}

#[cfg(test)]
mod tests {
    use lending_iterator::LendingIterator;

    use crate::{builder::DadaClientBuilder, tests::next_key};

    #[test]
    fn test_push() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).build().unwrap();
        let (_, mut dc) = client.split();

        let mut writer = dc.writer();

        assert_eq!(32, writer.push(&[0u8; 32]).unwrap());

        let mut reader = dc.reader();
        assert_eq!([0u8; 32], reader.next().unwrap().read_block());
    }

    #[test]
    fn test_push_pop_data() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).build().unwrap();
        assert_eq!(8, client.push_data(&[0u8; 8]).unwrap());
        assert_eq!(vec![0u8; 8], client.pop_data().unwrap());
    }
}
