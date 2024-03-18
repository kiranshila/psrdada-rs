use psrdada::prelude::*;
use std::io::{Read, Write};

fn main() {
    // Build the paired client
    let key = 0xb0ba;
    let mut client = DadaClientBuilder::new(key).build().unwrap();

    // Split into individual clients
    let (_, mut data_client) = client.split();

    // Construct the writer (mutable borrow), panicing if a lock is not obtainable
    let mut writer = data_client.writer().unwrap();

    // Grab the next block in the ring (assuming we can)
    let mut write_block = writer.next().unwrap();

    // Write using std::io::Write so you can write chunks at a time
    write_block.write_all(&[0u8; 10]).unwrap();

    // Inform the backend that we've completed writing
    write_block.commit();

    // Drop the writer to unlock it (this would happen also when the writer leaves scope)
    drop(writer);

    // Construct the reader (mutable borrow), panicing if a lock is not obtainable
    let mut reader = data_client.reader().unwrap();

    // Grab the next read block in the ring
    let mut read_block = reader.next().unwrap();

    // Read using std::io::Read
    let mut buf = [0u8; 10];
    read_block.read_exact(&mut buf).unwrap();
}
