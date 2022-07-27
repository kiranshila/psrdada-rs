# psrdada-rs

A Rust high level wrapper around the
[psrdada](http://psrdada.sourceforge.net/) shared memory ring buffer library,
common in radio astronomy.

PSRDada itself is completley undocumented and yet is mysteriously popular in radio astronomy.
This library tries to ascertain how the source is used, but may be incorrect. As such, use
at your own risk.

This crate will provide a "safeish" interface, trying to minimize the probability
of memory errors. `psrdada-sys`, also provided here, is the
[bindgen](https://github.com/rust-lang/rust-bindgen)-produced raw rust bindings.
To minimize effort, the `psrdada` library artifact is built during compile time
of the bindings using the [cc](https://docs.rs/cc/latest/cc/) crate. All that's
required is that you have a working C compiler.

Unlike the upstream package, we'll try to keep things documented and tested as
this could hopefully be used in "production" code.
