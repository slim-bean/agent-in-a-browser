//! Support types for the select! proc macro.
//!
//! The proc macro lives in wasi-tokio-macros; this module provides
//! runtime helpers used by the generated code.

/// Send+Sync raw pointer wrapper for use in select! generated code.
/// SAFETY: single-threaded WASM — no concurrent access is possible.
#[doc(hidden)]
pub struct SendPtr<T>(*mut T);

impl<T> SendPtr<T> {
    #[inline]
    pub fn new(ptr: *mut T) -> Self {
        Self(ptr)
    }

    #[inline]
    pub fn get(&self) -> *mut T {
        self.0
    }
}

unsafe impl<T> Send for SendPtr<T> {}
unsafe impl<T> Sync for SendPtr<T> {}
