//! Task utilities matching tokio_util::task.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// TaskTracker — simplified for single-threaded WASM.
#[derive(Clone, Default)]
pub struct TaskTracker {
    count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl TaskTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn spawn<F>(&self, future: F) -> TaskTrackerToken
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // In WASM, run to completion inline
        let _ = tokio::spawn(future);
        let count = self.count.clone();
        TaskTrackerToken { count }
    }

    pub fn close(&self) {}

    pub async fn wait(&self) {}

    pub fn len(&self) -> usize {
        self.count.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub struct TaskTrackerToken {
    count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Drop for TaskTrackerToken {
    fn drop(&mut self) {
        self.count
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// AbortOnDropHandle — wraps a JoinHandle and aborts on drop.
pub struct AbortOnDropHandle<T> {
    handle: tokio::JoinHandle<T>,
}

impl<T> AbortOnDropHandle<T> {
    pub fn new(handle: tokio::JoinHandle<T>) -> Self {
        Self { handle }
    }

    /// Abort the underlying task. In WASM, tasks complete inline so this is a no-op.
    pub fn abort(&self) {
        self.handle.abort();
    }

    /// Check if the task is finished. In WASM, always true.
    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }
}

impl<T> std::fmt::Debug for AbortOnDropHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AbortOnDropHandle").finish()
    }
}

impl<T: Unpin> Future for AbortOnDropHandle<T> {
    type Output = Result<T, tokio::JoinError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.handle).poll(cx)
    }
}

impl<T> Drop for AbortOnDropHandle<T> {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
