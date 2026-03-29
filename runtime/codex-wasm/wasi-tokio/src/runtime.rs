//! Runtime types matching tokio::runtime.
//!
//! In WASM single-threaded mode, the "runtime" is just the main thread.

/// The flavor of a runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeFlavor {
    CurrentThread,
    MultiThread,
}

/// Handle to the tokio runtime — stub for WASM.
pub struct Handle;

impl Handle {
    /// Try to get a handle to the current runtime.
    /// In WASM, there's always a "runtime" (we're always on the main thread).
    pub fn try_current() -> Result<Self, TryCurrentError> {
        Ok(Handle)
    }

    /// Get a handle to the current runtime, panicking if not in a runtime context.
    pub fn current() -> Self {
        Handle
    }

    /// Enter the runtime context.
    pub fn enter(&self) -> EnterGuard<'_> {
        EnterGuard { _handle: self }
    }

    /// Block on a future within this runtime.
    pub fn block_on<F: std::future::Future>(&self, future: F) -> F::Output {
        crate::block_on(future)
    }

    /// Spawn a future on this runtime.
    pub fn spawn<F>(&self, future: F) -> crate::JoinHandle<F::Output>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        crate::spawn(future)
    }

    /// Spawn a blocking task. In single-threaded WASM, this runs inline.
    pub fn spawn_blocking<F, R>(&self, f: F) -> crate::JoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        let result = f();
        let slot = std::sync::Arc::new(std::sync::Mutex::new(Some(result)));
        crate::JoinHandle {
            result_slot: Some(slot),
        }
    }

    /// Get the runtime flavor. In WASM, it's always CurrentThread.
    pub fn runtime_flavor(&self) -> RuntimeFlavor {
        RuntimeFlavor::CurrentThread
    }
}

pub struct EnterGuard<'a> {
    _handle: &'a Handle,
}

#[derive(Debug)]
pub struct TryCurrentError;

impl std::fmt::Display for TryCurrentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no current runtime")
    }
}

impl std::error::Error for TryCurrentError {}

/// Runtime builder — stub for WASM.
pub struct Builder;

impl Builder {
    pub fn new_current_thread() -> Self {
        Builder
    }

    pub fn new_multi_thread() -> Self {
        Builder
    }

    pub fn enable_all(&mut self) -> &mut Self {
        self
    }

    pub fn enable_io(&mut self) -> &mut Self {
        self
    }

    pub fn enable_time(&mut self) -> &mut Self {
        self
    }

    pub fn worker_threads(&mut self, _val: usize) -> &mut Self {
        self
    }

    pub fn build(&mut self) -> std::io::Result<Runtime> {
        Ok(Runtime)
    }
}

/// Runtime — stub for WASM.
pub struct Runtime;

impl Runtime {
    pub fn handle(&self) -> Handle {
        Handle
    }

    pub fn block_on<F: std::future::Future>(&self, future: F) -> F::Output {
        crate::block_on(future)
    }

    pub fn spawn<F>(&self, future: F) -> crate::JoinHandle<F::Output>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        crate::spawn(future)
    }
}
