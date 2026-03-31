//! Pluggable HTTP backend for wasi-reqwest.
//!
//! The WASM component entry point (codex-wasm-agent) registers a concrete
//! backend that routes through wasi:http/outgoing-handler. Library crates
//! call `execute_request()` which dispatches to the registered backend.

use std::sync::OnceLock;

use crate::{Error, Result};

/// Raw HTTP request passed to the backend.
pub struct RawRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

/// Raw HTTP response from the backend (fully buffered body).
pub struct RawResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Streaming HTTP response — headers available immediately, body read on demand.
pub struct RawStreamingResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// Produces body chunks on demand. Returns Ok(chunk) for data,
    /// Ok(empty) for end-of-stream, Err for errors.
    pub body_reader: Box<dyn BodyChunkReader>,
}

/// Trait for reading body chunks incrementally.
/// Each call to `read_chunk` JSPI-suspends until data arrives or stream closes.
pub trait BodyChunkReader: Send {
    /// Read the next chunk of up to `max_len` bytes.
    /// Blocks (JSPI-suspends) until data is available.
    /// Returns Ok(data) with data, Ok(empty) for EOF, Err for stream errors.
    fn read_chunk(&self, max_len: usize) -> std::result::Result<Vec<u8>, String>;
}

/// Trait for HTTP backends.
pub trait HttpBackend: Send + Sync + 'static {
    /// Execute a request and return the full response (body fully buffered).
    fn execute(&self, request: RawRequest) -> std::result::Result<RawResponse, String>;

    /// Execute a request and return a streaming response.
    /// Headers are available immediately; body is read chunk-by-chunk.
    /// Default implementation falls back to `execute()` (full buffering).
    fn execute_streaming(
        &self,
        request: RawRequest,
    ) -> std::result::Result<RawStreamingResponse, String> {
        let resp = self.execute(request)?;
        Ok(RawStreamingResponse {
            status: resp.status,
            headers: resp.headers,
            body_reader: Box::new(BufferedBodyReader {
                data: std::sync::Mutex::new(Some(resp.body)),
            }),
        })
    }
}

/// BodyChunkReader that yields a pre-buffered body as a single chunk.
struct BufferedBodyReader {
    data: std::sync::Mutex<Option<Vec<u8>>>,
}

impl BodyChunkReader for BufferedBodyReader {
    fn read_chunk(&self, _max_len: usize) -> std::result::Result<Vec<u8>, String> {
        let mut guard = self.data.lock().unwrap_or_else(|e| e.into_inner());
        Ok(guard.take().unwrap_or_default())
    }
}

/// Global backend instance, set once at component startup.
static BACKEND: OnceLock<Box<dyn HttpBackend>> = OnceLock::new();

/// Register the HTTP backend. Called once by the component entry point.
pub fn set_backend(backend: impl HttpBackend) {
    let _ = BACKEND.set(Box::new(backend));
}

/// Execute an HTTP request through the registered backend.
pub fn execute_request(request: RawRequest) -> Result<RawResponse> {
    let backend = BACKEND.get().ok_or_else(|| {
        Error::new("HTTP backend not initialized — call reqwest::backend::set_backend() first")
    })?;
    backend.execute(request).map_err(Error::new)
}

/// Execute a streaming HTTP request through the registered backend.
pub fn execute_streaming_request(request: RawRequest) -> Result<RawStreamingResponse> {
    let backend = BACKEND.get().ok_or_else(|| {
        Error::new("HTTP backend not initialized — call reqwest::backend::set_backend() first")
    })?;
    backend.execute_streaming(request).map_err(Error::new)
}
