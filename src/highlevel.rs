use std::io::Write;

use crate::{
    errors::{PsrdadaError, PsrdadaResult},
    io::{BlockType, WriteHalf},
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

#[cfg(test)]
mod tests {
    use lending_iterator::LendingIterator;

    use crate::{builder::DadaClientBuilder, tests::next_key};

    use super::*;
    #[test]
    fn test_push() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).build().unwrap();
        let (mut read, mut write) = client.split(BlockType::Data);
        assert_eq!(32, write.push(&[0u8; 32]).unwrap());
        assert_eq!([0u8; 32], read.next().unwrap().read_block());
    }
}
