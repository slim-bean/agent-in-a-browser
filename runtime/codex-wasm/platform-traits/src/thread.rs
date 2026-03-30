//! Drop-in replacement for `std::thread` in WASM.
//!
//! Provides `spawn`, `sleep`, `Builder`, and `JoinHandle` that match
//! `std::thread`'s API but work in single-threaded WASM:
//! - `spawn` runs the closure via `tokio::spawn` (cooperative async)
//! - `sleep` delegates to WASI clocks
//! - `JoinHandle::join()` returns the result if the task completed

use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Handle to a spawned background task.
pub struct JoinHandle<T> {
    result: Arc<Mutex<Option<T>>>,
}

impl<T> JoinHandle<T> {
    /// Wait for the task to complete and return its result.
    /// In WASM, tasks run cooperatively — if the task hasn't completed,
    /// this returns an error rather than blocking.
    pub fn join(self) -> Result<T, Box<dyn std::any::Any + Send>> {
        match self.result.lock().unwrap_or_else(|e| e.into_inner()).take() {
            Some(val) => Ok(val),
            None => Err(Box::new(
                "task has not completed (WASM cooperative scheduling)",
            )),
        }
    }

    /// Check if the task is finished.
    pub fn is_finished(&self) -> bool {
        self.result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }
}

/// Spawn a closure as a background task. In WASM, this queues it onto
/// the tokio task queue where `block_on` polls it cooperatively.
pub fn spawn<F, T>(f: F) -> JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let result: Arc<Mutex<Option<T>>> = Arc::new(Mutex::new(None));
    let result_clone = result.clone();

    tokio::spawn(async move {
        // Run the closure, catching panics to avoid killing the WASM module
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
            Ok(val) => {
                *result_clone.lock().unwrap_or_else(|e| e.into_inner()) = Some(val);
            }
            Err(e) => {
                console_log::console_error!(
                    "[wasm_thread::spawn] closure panicked: {:?}",
                    e.downcast_ref::<&str>().unwrap_or(&"unknown")
                );
            }
        }
    });

    JoinHandle { result }
}

/// Sleep the current thread. In WASM, this yields to the JS event loop
/// via WASI clocks for the specified duration.
pub fn sleep(dur: Duration) {
    // Use std::thread::sleep which in WASI maps to clock_sleep
    std::thread::sleep(dur);
}

/// Thread builder matching `std::thread::Builder`.
pub struct Builder {
    _name: Option<String>,
}

impl Builder {
    pub fn new() -> Self {
        Self { _name: None }
    }

    pub fn name(mut self, name: String) -> Self {
        self._name = Some(name);
        self
    }

    pub fn stack_size(self, _size: usize) -> Self {
        self // Ignored in WASM
    }

    pub fn spawn<F, T>(self, f: F) -> std::io::Result<JoinHandle<T>>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        Ok(spawn(f))
    }
}

/// Returns the current thread's name. Stub for WASM.
pub fn current() -> CurrentThread {
    CurrentThread
}

pub struct CurrentThread;

impl CurrentThread {
    pub fn name(&self) -> Option<&str> {
        Some("main")
    }

    pub fn id(&self) -> ThreadId {
        ThreadId(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ThreadId(u64);
