//! Synchronization utilities matching tokio_util::sync.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

/// CancellationToken — simplified for single-threaded WASM.
#[derive(Clone)]
pub struct CancellationToken {
    inner: Arc<CancellationTokenInner>,
}

struct CancellationTokenInner {
    cancelled: AtomicBool,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CancellationTokenInner {
                cancelled: AtomicBool::new(false),
            }),
        }
    }

    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Relaxed)
    }

    pub fn cancelled(&self) -> WaitForCancellationFuture {
        WaitForCancellationFuture {
            token: self.clone(),
        }
    }

    pub fn child_token(&self) -> Self {
        // In single-threaded WASM, child tokens share the same inner state
        self.clone()
    }

    pub fn drop_guard(self) -> DropGuard {
        DropGuard { token: self }
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CancellationToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CancellationToken")
            .field("is_cancelled", &self.is_cancelled())
            .finish()
    }
}

pub struct WaitForCancellationFuture {
    token: CancellationToken,
}

impl Future for WaitForCancellationFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        if self.token.is_cancelled() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

/// Guard that cancels the token when dropped.
#[derive(Debug)]
pub struct DropGuard {
    token: CancellationToken,
}

impl Drop for DropGuard {
    fn drop(&mut self) {
        self.token.cancel();
    }
}

/// A boxed future that can be reused by replacing its inner future.
/// Minimal implementation for single-threaded WASM environments.
pub struct ReusableBoxFuture<'a, T> {
    inner: Pin<Box<dyn Future<Output = T> + Send + 'a>>,
}

impl<'a, T> ReusableBoxFuture<'a, T> {
    pub fn new<F: Future<Output = T> + Send + 'a>(future: F) -> Self {
        Self {
            inner: Box::pin(future),
        }
    }

    pub fn set<F: Future<Output = T> + Send + 'a>(&mut self, future: F) {
        self.inner = Box::pin(future);
    }

    pub fn get_pin(&mut self) -> Pin<&mut (dyn Future<Output = T> + Send)> {
        self.inner.as_mut()
    }

    pub async fn get(&mut self) -> T {
        (&mut *self).await
    }

    /// Poll the inner future directly (called by tokio-stream).
    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<T> {
        self.inner.as_mut().poll(cx)
    }
}

impl<T> Future for ReusableBoxFuture<'_, T> {
    type Output = T;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        self.inner.as_mut().poll(cx)
    }
}
