#![allow(dead_code)]
//! WASM shim for tiny_http — channel-based bridge for incoming HTTP requests.
//!
//! The host's `wasi:http/incoming-handler` export pushes requests into a global
//! channel via [`push_incoming_request`].  The login server (or any consumer)
//! calls [`Server::recv()`] which blocks on the channel receiver.  In
//! wasm32-wasip2 with JSPI the block transparently suspends until data arrives.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Global channel
// ---------------------------------------------------------------------------

/// Incoming HTTP request delivered from the host's incoming-handler export.
pub struct IncomingRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

type Channel = (
    Mutex<mpsc::Sender<IncomingRequest>>,
    Mutex<mpsc::Receiver<IncomingRequest>>,
);

static INCOMING_CHANNEL: OnceLock<Channel> = OnceLock::new();
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

fn channel() -> &'static Channel {
    INCOMING_CHANNEL.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        (Mutex::new(tx), Mutex::new(rx))
    })
}

/// Push an HTTP request into the channel so that [`Server::recv()`] can
/// consume it.  Called by the main component's incoming-handler export.
pub fn push_incoming_request(
    method: &str,
    path: &str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
) {
    let (tx_lock, _) = channel();
    if let Ok(tx) = tx_lock.lock() {
        let _ = tx.send(IncomingRequest {
            method: method.to_string(),
            path: path.to_string(),
            headers,
            body,
        });
    }
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

pub struct Server;

impl Server {
    /// Create a new server.  The address is ignored in WASM — all requests
    /// arrive through the incoming-handler channel.
    pub fn http<A: std::net::ToSocketAddrs>(_addr: A) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Reset shutdown flag for a fresh server session.
        SHUTDOWN.store(false, Ordering::Release);
        // Ensure the channel is initialised.
        let _ = channel();
        Ok(Server)
    }

    pub fn server_addr(&self) -> ListenAddr {
        ListenAddr
    }

    /// Block until the next HTTP request is available.
    ///
    /// Uses a poll-sleep loop instead of blocking `mpsc::recv()`.  Each
    /// `std::thread::sleep` call maps to `wasi:clocks/monotonic-clock` which
    /// JSPI-suspends, letting other async tasks (like the incoming-handler
    /// that feeds the channel) make progress.
    pub fn recv(&self) -> Result<Request, io::Error> {
        let (_, rx_lock) = channel();
        loop {
            let rx = rx_lock
                .lock()
                .map_err(|e| io::Error::other(format!("channel lock poisoned: {e}")))?;
            match rx.try_recv() {
                Ok(incoming) => return Ok(Request::from_incoming(incoming)),
                Err(mpsc::TryRecvError::Empty) => {
                    // Drop the lock before sleeping so push_incoming_request can acquire it.
                    drop(rx);
                    // Check shutdown flag before sleeping.
                    if SHUTDOWN.load(Ordering::Acquire) {
                        return Err(io::Error::other("server shutdown requested"));
                    }
                    // Sleep via WASI clocks — this JSPI-suspends, letting the
                    // incoming-handler deliver requests to the channel.
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(io::Error::other("channel disconnected"));
                }
            }
        }
    }

    /// Non-blocking receive — returns `Ok(None)` when no request is queued.
    pub fn try_recv(&self) -> Result<Option<Request>, io::Error> {
        let (_, rx_lock) = channel();
        let rx = rx_lock
            .lock()
            .map_err(|e| io::Error::other(format!("channel lock poisoned: {e}")))?;
        match rx.try_recv() {
            Ok(incoming) => Ok(Some(Request::from_incoming(incoming))),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(io::Error::other("channel disconnected"))
            }
        }
    }

    /// Unblock any pending `recv()` by setting the shutdown flag.
    /// The poll-sleep loop checks this flag and returns an error when set.
    pub fn unblock(&self) {
        SHUTDOWN.store(true, Ordering::Release);
    }
}

// ---------------------------------------------------------------------------
// ListenAddr
// ---------------------------------------------------------------------------

pub struct ListenAddr;

impl ListenAddr {
    pub fn to_ip(&self) -> Option<std::net::SocketAddr> {
        Some(std::net::SocketAddr::from(([127, 0, 0, 1], 0)))
    }
}

// ---------------------------------------------------------------------------
// Request
// ---------------------------------------------------------------------------

pub struct Request {
    method: Method,
    path: String,
    headers: Vec<Header>,
    body: Vec<u8>,
}

impl Request {
    fn from_incoming(inc: IncomingRequest) -> Self {
        let method = match inc.method.to_uppercase().as_str() {
            "GET" => Method::Get,
            "POST" => Method::Post,
            other => Method::Other(other.to_string()),
        };
        let headers = inc
            .headers
            .into_iter()
            .map(|(k, v)| Header {
                field: HeaderField(k),
                value: v,
            })
            .collect();
        Self {
            method,
            path: inc.path,
            headers,
            body: inc.body,
        }
    }

    pub fn url(&self) -> &str {
        &self.path
    }

    pub fn method(&self) -> &Method {
        &self.method
    }

    pub fn headers(&self) -> &[Header] {
        &self.headers
    }

    /// Content length derived from the body bytes we already have.
    pub fn body_length(&self) -> Option<usize> {
        Some(self.body.len())
    }

    /// Return an `io::Read` reader over the request body.
    pub fn as_reader(&self) -> impl io::Read + '_ {
        io::Cursor::new(&self.body)
    }

    /// Accept a `Response` — in this shim the actual HTTP response is handled
    /// by the host's incoming-handler, so this is a no-op.
    pub fn respond<R: io::Read>(&self, _response: Response<R>) -> io::Result<()> {
        Ok(())
    }

    pub fn into_writer(self) -> ResponseWriter {
        ResponseWriter
    }
}

// ---------------------------------------------------------------------------
// ResponseWriter
// ---------------------------------------------------------------------------

pub struct ResponseWriter;

impl io::Write for ResponseWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Method
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub enum Method {
    Get,
    Post,
    Other(String),
}

// ---------------------------------------------------------------------------
// Response
// ---------------------------------------------------------------------------

pub struct Response<R> {
    _reader: Option<R>,
    _status_code: StatusCode,
    _headers: Vec<Header>,
}

impl Response<io::Empty> {
    pub fn empty(status_code: impl Into<StatusCode>) -> Self {
        Response {
            _reader: None,
            _status_code: status_code.into(),
            _headers: Vec::new(),
        }
    }
}

impl Response<io::Cursor<Vec<u8>>> {
    pub fn from_string(s: impl Into<String>) -> Self {
        let bytes = s.into().into_bytes();
        Response {
            _reader: Some(io::Cursor::new(bytes)),
            _status_code: StatusCode(200),
            _headers: Vec::new(),
        }
    }

    pub fn from_data(data: Vec<u8>) -> Self {
        Response {
            _reader: Some(io::Cursor::new(data)),
            _status_code: StatusCode(200),
            _headers: Vec::new(),
        }
    }
}

impl<R: io::Read> Response<R> {
    pub fn with_status_code(mut self, code: impl Into<StatusCode>) -> Self {
        self._status_code = code.into();
        self
    }

    pub fn with_header(mut self, header: Header) -> Self {
        self._headers.push(header);
        self
    }
}

// ---------------------------------------------------------------------------
// StatusCode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct StatusCode(pub u16);

impl From<u16> for StatusCode {
    fn from(code: u16) -> Self {
        StatusCode(code)
    }
}

impl From<StatusCode> for u16 {
    fn from(code: StatusCode) -> Self {
        code.0
    }
}

impl StatusCode {
    pub fn default_reason_phrase(&self) -> &'static str {
        match self.0 {
            200 => "OK",
            301 => "Moved Permanently",
            302 => "Found",
            400 => "Bad Request",
            404 => "Not Found",
            500 => "Internal Server Error",
            _ => "Unknown",
        }
    }
}

// ---------------------------------------------------------------------------
// Header / HeaderField
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct HeaderField(String);

impl HeaderField {
    pub fn equiv(&self, other: &str) -> bool {
        self.0.eq_ignore_ascii_case(other)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for HeaderField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone)]
pub struct Header {
    pub field: HeaderField,
    pub value: String,
}

impl Header {
    pub fn from_bytes(field: &[u8], value: &[u8]) -> Result<Self, ()> {
        Ok(Header {
            field: HeaderField(String::from_utf8_lossy(field).to_string()),
            value: String::from_utf8_lossy(value).to_string(),
        })
    }
}
