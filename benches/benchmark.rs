use iai::{black_box, main};
use lending_iterator::prelude::*;
use psrdada::{DadaDB, DadaDBBuilder};
use rand::distributions::Standard;
use rand::prelude::*;

// I don't know if this gives us much info

fn tx_rx(hdu: &DadaDB, data: &[i8]) {
    let (mut reader, mut writer) = hdu.split();
    // Back and forth
    writer.push(data).unwrap();
    reader.next().unwrap().unwrap();
}

fn bench_tx_rx() {
    let data: Vec<i8> = rand::thread_rng().sample_iter(Standard).take(10).collect();
    let hdu = DadaDBBuilder::new(rand::thread_rng().gen(), "bench")
        .buf_size(10)
        .num_bufs(1)
        .build(true)
        .unwrap();
    tx_rx(&hdu, &data);
}

iai::main!(bench_tx_rx);
