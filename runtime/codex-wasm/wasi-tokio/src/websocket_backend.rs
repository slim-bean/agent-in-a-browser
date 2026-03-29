//! Pluggable WebSocket backend for wasi-tokio.
//!
//! The WASM component entry point (codex-wasm-tui) registers a concrete
//! backend that routes through the WIT websocket interface. Library crates
//! call the module-level functions which dispatch to the registered backend.

use std::sync::OnceLock;

/// Trait for WebSocket backends.
pub trait WebSocketBackend: Send + Sync + 'static {
    /// Open a WebSocket connection. Returns a handle on success.
    fn connect(&self, url: &str, protocols: &[String]) -> Result<u32, String>;

    /// Send a text message on the WebSocket.
    fn send(&self, handle: u32, data: &str) -> Result<(), String>;

    /// Receive the next text message. Returns None when the connection is closed.
    fn recv(&self, handle: u32) -> Result<Option<String>, String>;

    /// Close the WebSocket connection.
    fn close(&self, handle: u32);

    /// Check if the connection is closed.
    fn is_closed(&self, handle: u32) -> bool;
}

/// Global backend instance, set once at component startup.
static BACKEND: OnceLock<Box<dyn WebSocketBackend>> = OnceLock::new();

/// Register the WebSocket backend. Called once by the component entry point.
pub fn set_backend(backend: impl WebSocketBackend) {
    let _ = BACKEND.set(Box::new(backend));
}

fn get_backend() -> Result<&'static dyn WebSocketBackend, String> {
    BACKEND
        .get()
        .map(|b| b.as_ref())
        .ok_or_else(|| "WebSocket backend not initialized".to_string())
}

/// Open a WebSocket connection through the registered backend.
pub fn connect(url: &str, protocols: &[String]) -> Result<u32, String> {
    get_backend()?.connect(url, protocols)
}

/// Send a text message through the registered backend.
pub fn send(handle: u32, data: &str) -> Result<(), String> {
    get_backend()?.send(handle, data)
}

/// Receive the next text message through the registered backend.
pub fn recv(handle: u32) -> Result<Option<String>, String> {
    get_backend()?.recv(handle)
}

/// Close the connection through the registered backend.
pub fn close(handle: u32) {
    if let Ok(backend) = get_backend() {
        backend.close(handle);
    }
}

/// Check if the connection is closed.
pub fn is_closed(handle: u32) -> bool {
    get_backend().map(|b| b.is_closed(handle)).unwrap_or(true)
}
