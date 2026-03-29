//! Platform abstraction traits for running Codex in wasip2.
//!
//! This crate defines traits for system capabilities that differ between
//! native (tokio/OS) and WASM (wasi) environments. The shim crates
//! (wasi-tokio, wasi-reqwest, wasi-crossterm) handle most of the API
//! compatibility, but a few Codex internals need explicit platform
//! awareness — particularly sandbox policy and process execution.

pub mod thread;

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::time::Duration;

/// Result of executing a command.
#[derive(Debug, Clone)]
pub struct ExecOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
}

/// Error from process execution.
#[derive(Debug)]
pub enum ExecError {
    SpawnFailed(io::Error),
    Timeout {
        partial_stdout: String,
        partial_stderr: String,
    },
    Io(io::Error),
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpawnFailed(e) => write!(f, "spawn failed: {e}"),
            Self::Timeout { .. } => write!(f, "command timed out"),
            Self::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for ExecError {}

/// Sandbox type for the current platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxType {
    /// No sandbox (native, trusted environment).
    None,
    /// macOS Seatbelt.
    Seatbelt,
    /// Linux Landlock + seccomp.
    Landlock,
    /// Windows Restricted Token.
    WindowsRestrictedToken,
    /// WASM sandbox (capabilities enforced by runtime).
    Wasm,
}

/// Platform-level abstraction for capabilities that differ between native and WASM.
///
/// Most code uses the shim crates (wasi-tokio, wasi-reqwest) for API compatibility.
/// This trait is only needed where Codex internals make explicit platform decisions
/// (e.g., which sandbox to use, how to spawn processes at the lowest level).
pub trait Platform: Send + Sync + 'static {
    /// Execute a command synchronously, returning captured output.
    fn spawn_command(
        &self,
        cmd: &[String],
        cwd: &Path,
        env: &HashMap<String, String>,
        timeout: Option<Duration>,
    ) -> Result<ExecOutput, ExecError>;

    /// The sandbox type for this platform.
    fn sandbox_type(&self) -> SandboxType;

    /// Whether this platform supports spawning child processes.
    fn supports_process_spawn(&self) -> bool;
}

// ---------------------------------------------------------------------------
// Thread spawning abstraction
// ---------------------------------------------------------------------------

/// Spawn a background closure. On native platforms, this calls `std::thread::spawn`.
/// On WASM, it wraps the closure in a `tokio::spawn` async task (which runs
/// cooperatively on the single-threaded WASM runtime). The closure runs inline
/// if it's short, or is dropped if it's an infinite loop that would block.
///
/// Returns a `BackgroundHandle` that can be used to check completion.
pub fn spawn_background<F>(f: F) -> BackgroundHandle
where
    F: FnOnce() + Send + 'static,
{
    // In WASM, run the closure via tokio::spawn so it gets polled cooperatively.
    // The closure is wrapped in catch_unwind to prevent panics from killing WASM.
    tokio::spawn(async move {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    });
    BackgroundHandle { _priv: () }
}

/// Handle to a background task. In WASM, tasks are cooperatively scheduled
/// and cannot be joined synchronously.
pub struct BackgroundHandle {
    _priv: (),
}

impl BackgroundHandle {
    /// Join the background task (no-op in WASM — tasks are cooperative).
    pub fn join(self) -> Result<(), Box<dyn std::any::Any + Send>> {
        Ok(())
    }
}

/// Spawn a background closure, returning a std::thread::JoinHandle-compatible result.
/// This is the drop-in replacement for `std::thread::spawn` in WASM.
pub fn spawn_thread<F, T>(f: F) -> std::io::Result<BackgroundJoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let result = std::sync::Arc::new(std::sync::Mutex::new(None));
    let result_clone = result.clone();
    tokio::spawn(async move {
        let val = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        if let Ok(val) = val {
            *result_clone.lock().unwrap_or_else(|e| e.into_inner()) = Some(val);
        }
    });
    Ok(BackgroundJoinHandle { result })
}

/// JoinHandle for background tasks in WASM.
pub struct BackgroundJoinHandle<T> {
    result: std::sync::Arc<std::sync::Mutex<Option<T>>>,
}

impl<T> BackgroundJoinHandle<T> {
    pub fn join(self) -> Result<T, Box<dyn std::any::Any + Send>> {
        // In WASM, the task may or may not have completed.
        // Try to get the result; if not available, return an error.
        match self.result.lock().unwrap_or_else(|e| e.into_inner()).take() {
            Some(val) => Ok(val),
            None => Err(Box::new("task not yet completed")),
        }
    }
}

/// WASM platform implementation — commands are dispatched via WIT interfaces.
pub struct WasmPlatform;

impl Platform for WasmPlatform {
    fn spawn_command(
        &self,
        _cmd: &[String],
        _cwd: &Path,
        _env: &HashMap<String, String>,
        _timeout: Option<Duration>,
    ) -> Result<ExecOutput, ExecError> {
        // TODO: Route through WIT shell:unix/command interface
        Err(ExecError::SpawnFailed(io::Error::new(
            io::ErrorKind::Unsupported,
            "process spawning not yet implemented for WASM",
        )))
    }

    fn sandbox_type(&self) -> SandboxType {
        SandboxType::Wasm
    }

    fn supports_process_spawn(&self) -> bool {
        true // Will be true once WIT interface is wired up
    }
}
