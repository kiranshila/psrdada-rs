//! # psrdada-rs
//!
//! This is a rust library around the [psrdada](http://psrdada.sourceforge.net/) library commonly used in radio astronomy.
//! Unfortunately, the C library is for the most part undocumented, so the behavior presented by this rust library is what
//! the authors have been able to ascertain by reading the original example code.
//! As such, this might not be a 1-to-1 implementation of the original use case.
//!
//! ## Usecase
//!
//! Use this library if you want a safe abstraction around working with psrdada.
//! As in, use this library if you need to interface with applications that are expecting psrdada buffers.
//! Do not use if you don't have to, as it (psrdada itself) isn't as performant or featurefull as other IPC libraries.
//!
//! ### Alternatives
//!
//! The rust library [shmem-ipc](https://github.com/diwic/shmem-ipc) has excellent performance over shmem, usefull for large
//! data transfers (like windows of spectal data). It creates shared ringbuffers, much like psrdada.
//! Interfacing with D-Bus is fine for signalling and headers.
//!
//! If you *need* CUDA support, [NVSHMEM](https://developer.nvidia.com/nvshmem)
//! is a thing that exists, and you should use it. Also, linux has [mkfifo](https://linux.die.net/man/3/mkfifo) which works fine with CUDA
//! as discussed [here](https://forums.developer.nvidia.com/t/gpu-inter-process-communications-ipc-question/35936/12).
//!
//! Lastly, there is [ipc-channel](https://github.com/servo/ipc-channel), which uses the Rust channel API over OS-native IPC abstractions.
//! It's a really nice library.
//!
//! In short, if you are constructing a pipeline from scratch, don't use psrdada.
//! There are more mature, documented, more performant alternatives.
//!
//! ## Installation
//!
//! We are building and linking the psrdada library as part of the build of this crate, which requires you have a working C compiler.
//! See the [cc](https://docs.rs/cc/latest/cc/) crate for more details. This includes building with CUDA support (even if you don't
//! have an NVIDIA graphics card).
//!
//! ## Safety
//!
//! The original library is intrinsically unsafe as it is written in C.
//!
//! ## What we learned about psrdada
//!
//! - Don't use `ipcio_t` or `dada_hdu`.
//!
//! They are wrappers around `ipcbuf_t` and have all sorts of undefined behavior.
//! Specifically, `ipcio_t` reimplemented stdlib `read` and `write` behavior, but in unsafe ways.
//! Our abstraction presented here reimplements the behavior, but with Rust's compile-time gauruntees.
//! `dada_hdu` combines two `ipcbuf_t`s, the header and data buffers.
//! However, doing so breaks CUDA support (for some reason) and messes up the signalling of successful reads.
//!
//! - "End of data" is more or less a meaningless flag.
//!
//! End of data doesn't prevent us from reading more data or writing more data. It is just a signal we can observe.
//! The iterator interface we provide will produce `None` if we run out of data, trying to be consistent with what that
//! might mean. Additionally, there is a very specific order in which eod is set and read. It *must* be set after `mark_filled`
//! and before `unlock_write`. It *must* be read after `mark_cleared` and before `unlock_read`. Any other ordering doesn't work.

pub mod builder;
pub mod client;
pub mod errors;
pub mod headers;
pub mod highlevel;
pub mod io;
#[cfg(test)]
mod tests;
