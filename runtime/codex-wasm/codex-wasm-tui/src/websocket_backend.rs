//! WebSocket backend that routes through the WIT websocket interface.
//!
//! The host (browser) implements `codex:tui/websocket` by delegating
//! to the browser's native WebSocket API.

use crate::bindings::codex::tui::websocket;
use tokio::websocket_backend::WebSocketBackend;

pub struct WasiWebSocketBackend;

impl WebSocketBackend for WasiWebSocketBackend {
    fn connect(&self, url: &str, protocols: &[String]) -> Result<u32, String> {
        console_log::console_log!("[wasi-ws] connect: {}", url);
        let result = websocket::connect(url, protocols);
        match &result {
            Ok(handle) => console_log::console_log!("[wasi-ws] connected: handle={}", handle),
            Err(e) => console_log::console_error!("[wasi-ws] connect failed: {}", e),
        }
        result
    }

    fn send(&self, handle: u32, data: &str) -> Result<(), String> {
        websocket::send(handle, data)
    }

    fn recv(&self, handle: u32) -> Result<Option<String>, String> {
        websocket::recv(handle)
    }

    fn close(&self, handle: u32) {
        console_log::console_log!("[wasi-ws] close: handle={}", handle);
        websocket::close(handle);
    }

    fn is_closed(&self, handle: u32) -> bool {
        websocket::is_closed(handle)
    }
}
