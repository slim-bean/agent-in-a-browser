//! Pluggable process execution backend for wasi-tokio.
//!
//! The WASM component entry point (codex-wasm-agent) registers a concrete
//! backend that routes through the WIT shell-exec interface. Library crates
//! call `execute()` which dispatches to the registered backend.

use std::sync::OnceLock;

/// Request to execute a process.
pub struct ExecRequest {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: Vec<(String, String)>,
    pub stdin: Option<Vec<u8>>,
    pub timeout_ms: Option<u32>,
}

/// Result of process execution.
pub struct ExecResponse {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Trait for process execution backends.
pub trait ProcessBackend: Send + Sync + 'static {
    fn execute(&self, request: ExecRequest) -> Result<ExecResponse, String>;
}

/// Global backend instance, set once at component startup.
static BACKEND: OnceLock<Box<dyn ProcessBackend>> = OnceLock::new();

/// Register the process backend. Called once by the component entry point.
pub fn set_backend(backend: impl ProcessBackend) {
    let _ = BACKEND.set(Box::new(backend));
}

/// Execute a process through the registered backend.
pub fn execute(request: ExecRequest) -> Result<ExecResponse, String> {
    let backend = BACKEND.get().ok_or(
        "Process backend not initialized — call tokio::process_backend::set_backend() first",
    )?;
    backend.execute(request)
}
