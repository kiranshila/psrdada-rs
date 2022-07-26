# psrdada-rs

A Rust high level wrapper around the
[psrdada](http://psrdada.sourceforge.net/) shared memory ring buffer library,
common in radio astronomy.

This crate will provide a "safeish" interface, trying to minimize the probability
of memory errors. `psrdada-sys`, also provided here, is the
[bindgen](https://github.com/rust-lang/rust-bindgen)-produced raw rust bindings.
To minimize effort, the `psrdada` library artifact is built during compile time
of the bindings using the [cc](https://docs.rs/cc/latest/cc/) crate. All that's
required there is that you have a working C compiler.

Unlike the upstream package, we'll try to keep things documented and tested as
this could hopefully be used in "production" code.

## Examples

This library will never be 100% safe, but we can try to deal with all the memory we manage.
To track the lifetimes of ringbuffers, an opaque newtype `DadaKey` is created. Instances of the
main struct `DadaDB` hold a reference to an instance of this type. As such, the "lifetime" of
the `DadaDB` is tied to the lifetime of the key. To add to this, the `Drop` trait is implemented for key, so when the key leaves scope, it uninitialized the memory that key refers to. Of course, the possibility still exists that other programs can remove that memory from under us, but if we stick to Rust, we can get compile-time guarantees that that memory is valid.

### This won't compile

Threads can outlive the calling context, so Rust can't guarantee that `&key` will be valid. In our case, this means
that data that `connected_hdu` points to may be invalid.

```rust
fn bad_multithread() {
    let key = DadaKey(0xdead);
    let my_hdu = DadaDBBuilder::new(&key, "test").build().unwrap();
    std::thread::spawn(|| {
        let connected_hdu = DadaDB::connect(&key, "test_thread").unwrap();
    });
}
```

### This will compile (on Rust 1.63.0)

For the specific case of multithreaded code, Rust 1.63.0 adds scoped threads that track lifetimes of the things
the threads refer to. You can read the RFC [here](https://rust-lang.github.io/rfcs/3151-scoped-threads.html).

In that case, this *will* work

```rust
fn good_multithread() {
    let key = DadaKey(0xdead);
    let my_hdu = DadaDBBuilder::new(&key, "test").build().unwrap();
    std::thread::scope(|s| {
        s.spawn(|| {
            let my_connected_hdu = DadaDB::connect(&key, "test_thread").unwrap();
        });
    });
}
```