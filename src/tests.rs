//! Utilities that help us test modules
#![cfg(test)]

use std::sync::atomic::AtomicI32;

static TEST_BUF_IDX: AtomicI32 = AtomicI32::new(2);

// Make sure the key we get in the tests is unique
// We increment by 2 because the header and data buffers are both keyed, one idx apart and we alloc them together.
pub fn next_key() -> i32 {
    TEST_BUF_IDX.fetch_add(2, std::sync::atomic::Ordering::SeqCst)
}
