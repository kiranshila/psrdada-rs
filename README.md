# psrdada-rs

[![license](https://img.shields.io/badge/license-Apache--2.0_OR_MIT-blue?style=flat-square)](#license)
[![docs](https://img.shields.io/docsrs/psrdada?logo=rust&style=flat-square)](https://docs.rs/psrdada/latest/psrdada/index.html)
[![rustc](https://img.shields.io/badge/rustc-1.57+-blue?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![build status](https://img.shields.io/github/workflow/status/kiranshila/psrdada-rs/CI/main?style=flat-square&logo=github)](https://github.com/kiranshila/psrdada-rs/actions)
[![Codecov](https://img.shields.io/codecov/c/github/kiranshila/psrdada-rs?style=flat-square)](https://app.codecov.io/gh/kiranshila/psrdada-rs)

This is a rust library around the [psrdada](http://psrdada.sourceforge.net/) library commonly used in radio astronomy.
Unfortunately, the C library is for the most part undocumented, so the behavior presented by this rust library is what
the authors have been able to ascertain by reading the original example code.
As such, this might not be a 1-to-1 implementation of the original use case.

## Usecase

Use this library if you want a safe abstraction around working with psrdada.
As in, use this library if you need to interface with applications that are expecting psrdada buffers.
Do not use if you don't have to, as it (psrdada itself) isn't as performant or featureful as other IPC libraries.

### Alternatives

The rust library [shmem-ipc](https://github.com/diwic/shmem-ipc) has excellent performance over shmem, useful for large
data transfers (like windows of spectral data). It creates shared ringbuffers, much like psrdada.
Interfacing with D-Bus is fine for signaling and headers.

If you _need_ CUDA support, [NVSHMEM](https://developer.nvidia.com/nvshmem)
is a thing that exists, and you should use it. Also, linux has [mkfifo](https://linux.die.net/man/3/mkfifo) which works fine with CUDA
as discussed [here](https://forums.developer.nvidia.com/t/gpu-inter-process-communications-ipc-question/35936/12).

Lastly, there is [ipc-channel](https://github.com/servo/ipc-channel), which uses the Rust channel API over OS-native IPC abstractions.
It's a really nice library.

In short, if you are constructing a pipeline from scratch, don't use psrdada.
There are more mature, documented, more performant alternatives.

## Installation

We are building and linking the psrdada library as part of the build of this crate, which requires you have a working C compiler.
See the [cc](https://docs.rs/cc/latest/cc/) crate for more details.

## Example

The most simple way to use this library is to use the top-level `push` and `pop` methods.

```rust
use std::collections::HashMap;
use psrdada::builder::DadaClientBuilder;

let key = 0xb0ba;
let mut client = DadaClientBuilder::new(key).build().unwrap();

let data = [0u8, 5u8, 10u8];
let header = HashMap::from([
    ("foo".to_owned(), "bar".to_owned()),
    ("baz".to_owned(), "buzz".to_owned()),
]);

client.push_data(&data).unwrap();
// Unsafe as we're not checking if the keys and values are valid
unsafe { client.push_header(&header).unwrap() };
```

Beyond this, you can `split` the `DadaClient` into separate clients for headers and data, which can then be read and written to/from.

## Safety

The original library is intrinsically unsafe as it is written in C. This library tries to ensure at compile time some of the things the
C library checks at runtime. For example, If you try to write to buffer while something else is trying to read (from the same `DadaClient`), this
would usually fail a lock. Instead, in this library, we use Rust's borrowing system to ensure you can't build both at the same time.

Take the following code as an example

```rust
use std::io::{Read, Write};

use lending_iterator::LendingIterator;
use psrdada::builder::DadaClientBuilder;

// Build the paired client
let key = 0xb0ba;
let mut client = DadaClientBuilder::new(key).build().unwrap();

// Split into individual clients
let (_, mut data_client) = client.split();

// Construct the writer (mutable borrow)
let mut writer = data_client.writer();

// Grab the next block in the ring (assuming we can)
let mut write_block = writer.next().unwrap();

// Write using std::io::Write so you can write chunks at a time
write_block.write_all(&[0u8; 10]).unwrap();
write_block.commit();

// Construct the reader (mutable borrow)
let mut reader = data_client.reader();

// Grab the next read block in the ring
let mut read_block = reader.next().unwrap();

// Read using std::io::Read
let mut buf = [0u8; 10];
read_block.read_exact(&mut buf).unwrap();
```

without that `write_block.commit()` line, this code would not compile as there still exist a write in progress.
Additionally, you can only ever `split` once, so you'll only ever have a single reader and writer for each type.

## What we learned about psrdada

- Don't use `ipcio_t` or `dada_hdu`.

They are wrappers around `ipcbuf_t` and have all sorts of undefined behavior.
Specifically, `ipcio_t` reimplemented stdlib `read` and `write` behavior, but in unsafe ways.
Our abstraction presented here reimplements the behavior, but with Rust's compile-time guarantees.
`dada_hdu` combines two `ipcbuf_t`s, the header and data buffers.
However, doing so breaks CUDA support (for some reason) and messes up the signaling of successful reads.

- "End of data" is more or less a meaningless flag.

End of data doesn't prevent us from reading more data or writing more data. It is just a signal we can observe.
The iterator interface we provide will produce `None` if we run out of data, trying to be consistent with what that
might mean. Additionally, there is a very specific order in which eod is set and read. It _must_ be set after `mark_filled`
and before `unlock_write`. It _must_ be read after `mark_cleared` and before `unlock_read`. Any other ordering doesn't work.

### License

psrdada-rs is distributed under the terms of both the MIT license and the Apache License (Version 2.0).

See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT) for details.
