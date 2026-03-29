//! Time utilities matching tokio_util::time.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Extension trait for adding timeout to futures.
pub trait FutureExt: Future + Sized {
    fn timeout(self, duration: std::time::Duration) -> tokio::time::Timeout<Self>
    where
        Self: Unpin,
    {
        tokio::time::timeout(duration, self)
    }
}

impl<F: Future> FutureExt for F {}
