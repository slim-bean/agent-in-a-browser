//! I/O utilities matching tokio_util::io.

use bytes::Bytes;
use futures_core::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

/// ReaderStream — converts a std::io::Read into a Stream of Bytes.
/// Simplified: reads all content eagerly (fine for WASM single-threaded).
pub struct ReaderStream {
    data: Option<Vec<u8>>,
}

impl ReaderStream {
    pub fn new<R: std::io::Read>(mut reader: R) -> Self {
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        Self { data: Some(buf) }
    }

    pub fn with_capacity<R: std::io::Read>(mut reader: R, _capacity: usize) -> Self {
        Self::new(reader)
    }
}

impl Stream for ReaderStream {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.data.take() {
            Some(data) if !data.is_empty() => Poll::Ready(Some(Ok(Bytes::from(data)))),
            _ => Poll::Ready(None),
        }
    }
}
