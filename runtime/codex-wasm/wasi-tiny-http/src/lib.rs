#![allow(dead_code)]
//! WASM shim for tiny_http — provides type stubs for codex-login server.rs.
//! The actual HTTP server functionality is handled by the WASM incoming-handler.

use std::io;

pub struct Server;

impl Server {
    pub fn http<A: std::net::ToSocketAddrs>(_addr: A) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Err("HTTP server not available in WASM (use incoming-handler)".into())
    }

    pub fn server_addr(&self) -> ListenAddr {
        ListenAddr
    }

    pub fn recv(&self) -> Result<Request, io::Error> {
        Err(io::Error::other("not available in WASM"))
    }

    pub fn try_recv(&self) -> Result<Option<Request>, io::Error> {
        Ok(None)
    }

    pub fn unblock(&self) {}
}

pub struct ListenAddr;

impl ListenAddr {
    pub fn to_ip(&self) -> Option<std::net::SocketAddr> {
        Some(std::net::SocketAddr::from(([127, 0, 0, 1], 0)))
    }
}

pub struct Request;

impl Request {
    pub fn url(&self) -> &str {
        "/"
    }

    pub fn method(&self) -> &Method {
        &Method::Get
    }

    pub fn respond<R: io::Read>(&self, _response: Response<R>) -> io::Result<()> {
        Ok(())
    }

    pub fn into_writer(self) -> ResponseWriter {
        ResponseWriter
    }
}

pub struct ResponseWriter;

impl io::Write for ResponseWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, PartialEq)]
pub enum Method {
    Get,
    Post,
    Other(String),
}

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
}

impl<R: io::Read> Response<R> {
    pub fn from_data(data: R) -> Self {
        Response {
            _reader: Some(data),
            _status_code: StatusCode(200),
            _headers: Vec::new(),
        }
    }

    pub fn with_status_code(mut self, code: impl Into<StatusCode>) -> Self {
        self._status_code = code.into();
        self
    }

    pub fn with_header(mut self, header: Header) -> Self {
        self._headers.push(header);
        self
    }
}

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
