//! wasip2-compatible shim for the tungstenite crate.
//!
//! Re-exports types from the tokio-tungstenite shim's `tungstenite` module
//! so that `use tungstenite::Error` and `use tungstenite::Message` resolve.

pub use std::fmt;

/// WebSocket error type (stub for type signatures).
#[derive(Debug)]
pub enum Error {
    Protocol(String),
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Protocol(msg) => write!(f, "WebSocket protocol error: {msg}"),
            Error::Io(err) => write!(f, "WebSocket I/O error: {err}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
    }
}

/// WebSocket message type.
#[derive(Debug, Clone)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(Option<CloseFrame>),
}

impl Message {
    pub fn is_text(&self) -> bool {
        matches!(self, Message::Text(_))
    }

    pub fn is_binary(&self) -> bool {
        matches!(self, Message::Binary(_))
    }

    pub fn into_text(self) -> Result<String, Error> {
        match self {
            Message::Text(s) => Ok(s),
            _ => Err(Error::Protocol("not a text message".into())),
        }
    }

    pub fn into_data(self) -> Vec<u8> {
        match self {
            Message::Text(s) => s.into_bytes(),
            Message::Binary(d) | Message::Ping(d) | Message::Pong(d) => d,
            Message::Close(_) => Vec::new(),
        }
    }
}

/// Close frame data.
#[derive(Debug, Clone)]
pub struct CloseFrame {
    pub code: u16,
    pub reason: String,
}
