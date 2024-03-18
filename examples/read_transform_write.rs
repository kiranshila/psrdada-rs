//! In this example, we connect to two existing ringbuffers, reading from one and writing to the other - performing a transformation between them.
//! We could transform the header as well, but in this example we'll ignore them.

use psrdada::prelude::*;

fn main() {
    let in_key = 0xB0BA;
    let out_key = 0xCAFE;

    // Connect to the two header/data paired buffers
    let mut in_client = HduClient::connect(in_key).unwrap();
    let mut out_client = HduClient::connect(out_key).unwrap();

    // Split these into their header/data pairs
    let (_, mut in_data) = in_client.split();
    let (_, mut out_data) = out_client.split();

    // Create the readers and writers
    let mut in_data_rdr = in_data.reader().unwrap();
    let mut out_data_wdr = out_data.writer().unwrap();

    // Loop forever on reading from the input, applying the transformation and writing to the output
    while let Some(mut read_block) = in_data_rdr.next() {
        // Get the next write block
        if let Some(mut write_block) = out_data_wdr.next() {
            let read_bytes = read_block.block();
            let write_bytes = write_block.block();
            // Transform, these are just slices now, so you can do whatever you want!
            // Here, we will do something per byte, but this could just as easily be over
            // reinterpretations of the bytes as arrays, structs, whatever.
            write_bytes.iter_mut().zip(read_bytes).for_each(|(x, y)| {
                // Double every byte
                *x = *y * 2;
            });
            // No need to lock, mark cleared, or anything like that. That's all implicit wil RAII.
        } else {
            println!("Errored on getting the next write block, perhaps that buffer was destroyed?");
            break;
        }
    }
}
