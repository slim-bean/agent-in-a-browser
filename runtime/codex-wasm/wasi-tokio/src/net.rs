//! Network types matching tokio::net.
//!
//! Stub implementations — networking in WASM goes through wasi:http,
//! not raw sockets.

use std::io;

pub struct TcpListener;

impl TcpListener {
    pub async fn bind(_addr: impl std::net::ToSocketAddrs) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "TCP not available in WASM — use wasi:http",
        ))
    }
}

pub struct TcpStream;

impl TcpStream {
    pub async fn connect(_addr: impl std::net::ToSocketAddrs) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "TCP not available in WASM — use wasi:http",
        ))
    }
}

pub struct UnixStream;
pub struct UnixListener;
