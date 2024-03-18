# psrdada-rs

[![license](https://img.shields.io/badge/license-Apache--2.0_OR_MIT-blue?style=flat-square)](#license)
[![docs](https://img.shields.io/docsrs/psrdada?logo=rust&style=flat-square)](https://docs.rs/psrdada/latest/psrdada/index.html)
[![rustc](https://img.shields.io/badge/rustc-1.60+-blue?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![build status](https://img.shields.io/github/actions/workflow/status/kiranshila/psrdada-rs/ci.yml?branch=main?style=flat-square&logo=github)](https://github.com/kiranshila/psrdada-rs/actions)
[![Codecov](https://img.shields.io/codecov/c/github/kiranshila/psrdada-rs?style=flat-square)](https://app.codecov.io/gh/kiranshila/psrdada-rs)

This is a rust library around the [psrdada](http://psrdada.sourceforge.net/) library commonly used in radio astronomy.
Unfortunately, the C library is for the most part undocumented, so the behavior presented by this rust library is what the authors have been able to ascertain by reading the original example code.
As such, this might not be a 1-to-1 implementation of the original use case and implements only a subset
of the features available in the C library.

## Installation

You need to build and install PSRDADA manually, following the installation guide found [here](https://psrdada.sourceforge.net/download.shtml).
Alternatively, you can use the [nix](https://nixos.org/) flake [here](https://github.com/kiranshila/psrdada.nix/blob/main/flake.nix) to declaratively create environments (shells/docker containers/operating systems) with PSRDADA baked in (deterministically).

## Safety

The original library is intrinsically unsafe as it is written in C, but also there are very few checks that the user uses it correctly.
This library tries to ensure at compile time some of the things the C library checks at runtime. For example, If you try to write to buffer
while something else is trying to read, this would usually fail a lock. Instead, in this library, we use Rust's borrowing system to ensure 
you can't build both at the same time. The same goes for read/write blocks. References to these cannot exist once you mark them as cleared.

This is a huge ergonomic improvement over the C library (and the C++ library to some extent, as they attempt to implement some RAII patterns).

Take the following code as an example

```rust
use psrdada::prelude::*;
use std::io::{Read, Write};

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
```

without that `write_block.commit()` line, this code would not compile as there still exists a write in progress.
Additionally, you can only ever `split` once, so you'll only ever have a single reader and writer for each type.

Please see the examples for some more use cases.

### Thanks

Much of the implementation is inspired by other "modern" wrappings of PSRDADA, especially [PSRDADA_CPP](https://gitlab.mpcdf.mpg.de/mpifr-bdg/psrdada_cpp).

### License

`psrdada-rs` is distributed under the terms of both the MIT license and the Apache License (Version 2.0).

See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT) for details.
