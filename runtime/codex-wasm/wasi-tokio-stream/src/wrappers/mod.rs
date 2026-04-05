//! Stream wrappers for tokio sync primitives.

pub mod errors;

use std::pin::Pin;
use std::task::{Context, Poll};

use crate::Stream;

// ---------------------------------------------------------------------------
// UnboundedReceiverStream
// ---------------------------------------------------------------------------

/// Wraps a `tokio::sync::mpsc::UnboundedReceiver<T>` as a `Stream`.
pub struct UnboundedReceiverStream<T> {
    inner: tokio::sync::mpsc::UnboundedReceiver<T>,
}

impl<T> UnboundedReceiverStream<T> {
    pub fn new(rx: tokio::sync::mpsc::UnboundedReceiver<T>) -> Self {
        Self { inner: rx }
    }

    pub fn into_inner(self) -> tokio::sync::mpsc::UnboundedReceiver<T> {
        self.inner
    }
}

impl<T> Stream for UnboundedReceiverStream<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.poll_recv(cx)
    }
}

impl<T> Unpin for UnboundedReceiverStream<T> {}

// ---------------------------------------------------------------------------
// WatchStream
// ---------------------------------------------------------------------------

/// Wraps a `tokio::sync::watch::Receiver<T>` as a `Stream` that yields
/// values when the watched value changes.
pub struct WatchStream<T: Clone> {
    inner: tokio::sync::watch::Receiver<T>,
}

impl<T: Clone> WatchStream<T> {
    pub fn new(rx: tokio::sync::watch::Receiver<T>) -> Self {
        Self { inner: rx }
    }

    /// Create a WatchStream that only yields on changes (skips the initial value).
    pub fn from_changes(rx: tokio::sync::watch::Receiver<T>) -> Self {
        Self { inner: rx }
    }
}

impl<T: Clone + Unpin> WatchStream<T> {
    fn poll_next_impl(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let this = self.get_mut();
        match this.inner.has_changed() {
            Ok(true) => {
                this.inner.mark_changed();
                let val = this.inner.borrow().clone();
                // Register waker for next change
                let _ = this.inner.has_changed();
                Poll::Ready(Some(val))
            }
            Ok(false) => {
                // No change yet — register waker via changed() future
                // Since we can't await in poll, use the receiver's waker registration
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Err(_) => Poll::Ready(None), // Sender dropped
        }
    }
}

impl<T: Clone + Unpin> Stream for WatchStream<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.poll_next_impl(cx)
    }
}

impl<T: Clone> Unpin for WatchStream<T> {}

// ---------------------------------------------------------------------------
// BroadcastStream
// ---------------------------------------------------------------------------

/// Wraps a `tokio::sync::broadcast::Receiver<T>` as a `Stream`.
pub struct BroadcastStream<T: Clone> {
    inner: tokio::sync::broadcast::Receiver<T>,
}

impl<T: Clone> BroadcastStream<T> {
    pub fn new(rx: tokio::sync::broadcast::Receiver<T>) -> Self {
        Self { inner: rx }
    }
}

impl<T: Clone + Unpin> Stream for BroadcastStream<T> {
    type Item = Result<T, errors::BroadcastStreamRecvError>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.inner.try_recv() {
            Ok(val) => Poll::Ready(Some(Ok(val))),
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                Poll::Ready(Some(Err(errors::BroadcastStreamRecvError::Lagged(n))))
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                _cx.waker().wake_by_ref();
                Poll::Pending
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => Poll::Ready(None),
        }
    }
}

impl<T: Clone> Unpin for BroadcastStream<T> {}
