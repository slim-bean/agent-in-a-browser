//! Task utilities matching tokio::task.

use std::future::Future;

// Re-export JoinHandle and JoinError so `tokio::task::*` works
pub use super::JoinError;
pub use super::JoinHandle;

/// spawn — delegates to the top-level `tokio::spawn`.
/// This allows `tokio::task::spawn()` to work in addition to `tokio::spawn()`.
pub fn spawn<F>(future: F) -> super::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    super::spawn(future)
}

/// spawn_blocking — in WASM, just runs the closure inline.
pub fn spawn_blocking<F, R>(f: F) -> super::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let result = f();
    let slot = std::sync::Arc::new(std::sync::Mutex::new(Some(result)));
    super::JoinHandle {
        result_slot: Some(slot),
    }
}

/// block_in_place — in WASM, just runs the closure inline (we're already blocking).
pub fn block_in_place<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    f()
}

/// yield_now — no-op in single-threaded WASM.
pub async fn yield_now() {}

/// JoinSet — simplified task set for WASM.
pub struct JoinSet<T> {
    results: Vec<T>,
}

impl<T: Send + 'static> JoinSet<T> {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
        }
    }

    pub fn spawn<F>(&mut self, future: F)
    where
        F: Future<Output = T> + Send + 'static,
    {
        let result = super::block_on(future);
        self.results.push(result);
    }

    pub async fn join_next(&mut self) -> Option<Result<T, super::JoinError>> {
        self.results.pop().map(Ok)
    }

    pub fn len(&self) -> usize {
        self.results.len()
    }

    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }

    pub fn abort_all(&mut self) {
        // Already completed in WASM
    }

    pub async fn join_all(mut self) -> Vec<T> {
        // In single-threaded WASM, all tasks already completed inline.
        // Drain in reverse to match join_next order.
        self.results.drain(..).collect()
    }
}
