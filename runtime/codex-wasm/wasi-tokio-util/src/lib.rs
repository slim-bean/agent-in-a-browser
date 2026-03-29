#![allow(dead_code, unused_variables, unused_imports, unused_mut)]
//! wasi-tokio-util: A tokio-util-compatible API shim for wasip2 environments.

pub mod bytes {
    //! Re-export core types from the `bytes` crate so that
    //! `tokio_util::bytes::{Buf, BufMut, Bytes, BytesMut}` resolves.
    pub use ::bytes::{Buf, BufMut, Bytes, BytesMut};
}

pub mod codec;
pub mod either;
pub mod io;
pub mod sync;
pub mod task;
pub mod time;
