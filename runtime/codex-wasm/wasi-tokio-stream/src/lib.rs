//! wasip2-compatible shim for tokio-stream.
//!
//! Provides the `Stream` trait re-export, `StreamExt` extension trait, and
//! `wrappers` module with `BroadcastStream`, `WatchStream`, and
//! `UnboundedReceiverStream` adapters backed by our wasi-tokio shim.

pub use futures_core::Stream;

pub mod wrappers;

/// Extension trait for `Stream` — provides `.next()` and `.fuse()`.
pub trait StreamExt: Stream {
    /// Returns the next item from the stream, or `None` if the stream is exhausted.
    fn next(&mut self) -> Next<'_, Self>
    where
        Self: Unpin,
    {
        Next { stream: self }
    }

    /// Wraps the stream in a `Fuse` adapter that yields `None` forever after
    /// the first `None`.
    fn fuse(self) -> Fuse<Self>
    where
        Self: Sized,
    {
        Fuse {
            stream: self,
            done: false,
        }
    }
}

impl<T: ?Sized> StreamExt for T where T: Stream {}

// --- Next future ---

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Future returned by `StreamExt::next()`.
pub struct Next<'a, S: ?Sized> {
    stream: &'a mut S,
}

impl<S: Stream + Unpin + ?Sized> Future for Next<'_, S> {
    type Output = Option<S::Item>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut *self.stream).poll_next(cx)
    }
}

// --- Fuse adapter ---

pin_project_lite::pin_project! {
    /// Stream adapter that yields `None` forever after the underlying stream
    /// returns `None` for the first time.
    pub struct Fuse<S> {
        #[pin]
        stream: S,
        done: bool,
    }
}

impl<S: Stream> Stream for Fuse<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        if *this.done {
            return Poll::Ready(None);
        }
        match this.stream.poll_next(cx) {
            Poll::Ready(None) => {
                *this.done = true;
                Poll::Ready(None)
            }
            other => other,
        }
    }
}
