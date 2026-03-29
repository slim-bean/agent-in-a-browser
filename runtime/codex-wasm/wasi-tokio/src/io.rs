//! I/O traits and utilities matching tokio::io.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

// Re-export std io types that tokio re-exports
pub use std::io::Error;
pub use std::io::ErrorKind;
pub use std::io::Result;

/// AsyncRead trait (simplified for WASM).
pub trait AsyncRead {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>>;
}

/// AsyncWrite trait (simplified for WASM).
pub trait AsyncWrite {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>>;

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>>;

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>>;
}

/// ReadBuf matching tokio::io::ReadBuf.
pub struct ReadBuf<'a> {
    buf: &'a mut [u8],
    filled: usize,
}

impl<'a> ReadBuf<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, filled: 0 }
    }

    pub fn filled(&self) -> &[u8] {
        &self.buf[..self.filled]
    }

    pub fn filled_mut(&mut self) -> &mut [u8] {
        &mut self.buf[..self.filled]
    }

    pub fn unfilled_mut(&mut self) -> &mut [u8] {
        &mut self.buf[self.filled..]
    }

    pub fn remaining(&self) -> usize {
        self.buf.len() - self.filled
    }

    pub fn put_slice(&mut self, src: &[u8]) {
        let len = src.len().min(self.remaining());
        self.buf[self.filled..self.filled + len].copy_from_slice(&src[..len]);
        self.filled += len;
    }

    pub fn advance(&mut self, n: usize) {
        self.filled += n;
    }
}

/// AsyncReadExt — extension trait with utility methods.
pub trait AsyncReadExt: AsyncRead {
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> ReadFuture<'a, Self>
    where
        Self: Unpin,
    {
        ReadFuture { reader: self, buf }
    }

    fn read_to_end<'a>(&'a mut self, buf: &'a mut Vec<u8>) -> ReadToEndFuture<'a, Self>
    where
        Self: Unpin,
    {
        ReadToEndFuture {
            reader: self,
            buf,
            total: 0,
        }
    }

    async fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize>
    where
        Self: Unpin,
    {
        let mut bytes = Vec::new();
        let n = self.read_to_end(&mut bytes).await?;
        match String::from_utf8(bytes) {
            Ok(s) => {
                buf.push_str(&s);
                Ok(n)
            }
            Err(e) => {
                buf.push_str(&String::from_utf8_lossy(e.as_bytes()));
                Ok(n)
            }
        }
    }
}

/// Future for AsyncReadExt::read()
pub struct ReadFuture<'a, R: ?Sized> {
    reader: &'a mut R,
    buf: &'a mut [u8],
}

impl<R: AsyncRead + Unpin + ?Sized> Future for ReadFuture<'_, R> {
    type Output = io::Result<usize>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        let mut read_buf = ReadBuf::new(this.buf);
        match Pin::new(&mut *this.reader).poll_read(cx, &mut read_buf) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(read_buf.filled)),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Future for AsyncReadExt::read_to_end()
pub struct ReadToEndFuture<'a, R: ?Sized> {
    reader: &'a mut R,
    buf: &'a mut Vec<u8>,
    total: usize,
}

impl<R: AsyncRead + Unpin + ?Sized> Future for ReadToEndFuture<'_, R> {
    type Output = io::Result<usize>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        loop {
            let mut tmp = [0u8; 8192];
            let mut read_buf = ReadBuf::new(&mut tmp);
            match Pin::new(&mut *this.reader).poll_read(cx, &mut read_buf) {
                Poll::Ready(Ok(())) => {
                    let n = read_buf.filled;
                    if n == 0 {
                        return Poll::Ready(Ok(this.total));
                    }
                    this.buf.extend_from_slice(&tmp[..n]);
                    this.total += n;
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<T: AsyncRead + ?Sized> AsyncReadExt for T {}

/// AsyncWriteExt — extension trait with utility methods.
pub trait AsyncWriteExt: AsyncWrite {
    fn write_all<'a>(&'a mut self, buf: &'a [u8]) -> WriteAllFuture<'a, Self>
    where
        Self: Unpin,
    {
        WriteAllFuture {
            writer: self,
            buf,
            pos: 0,
        }
    }

    fn flush(&mut self) -> FlushFuture<'_, Self>
    where
        Self: Unpin,
    {
        FlushFuture { writer: self }
    }

    fn shutdown(&mut self) -> ShutdownFuture<'_, Self>
    where
        Self: Unpin,
    {
        ShutdownFuture { writer: self }
    }
}

/// Future for AsyncWriteExt::write_all()
pub struct WriteAllFuture<'a, W: ?Sized> {
    writer: &'a mut W,
    buf: &'a [u8],
    pos: usize,
}

impl<W: AsyncWrite + Unpin + ?Sized> Future for WriteAllFuture<'_, W> {
    type Output = io::Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        while this.pos < this.buf.len() {
            match Pin::new(&mut *this.writer).poll_write(cx, &this.buf[this.pos..]) {
                Poll::Ready(Ok(n)) => {
                    if n == 0 {
                        return Poll::Ready(Err(io::Error::new(
                            io::ErrorKind::WriteZero,
                            "write returned 0",
                        )));
                    }
                    this.pos += n;
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(Ok(()))
    }
}

/// Future for AsyncWriteExt::flush()
pub struct FlushFuture<'a, W: ?Sized> {
    writer: &'a mut W,
}

impl<W: AsyncWrite + Unpin + ?Sized> Future for FlushFuture<'_, W> {
    type Output = io::Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut *self.writer).poll_flush(cx)
    }
}

/// Future for AsyncWriteExt::shutdown()
pub struct ShutdownFuture<'a, W: ?Sized> {
    writer: &'a mut W,
}

impl<W: AsyncWrite + Unpin + ?Sized> Future for ShutdownFuture<'_, W> {
    type Output = io::Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut *self.writer).poll_shutdown(cx)
    }
}

impl<T: AsyncWrite + ?Sized> AsyncWriteExt for T {}

/// BufReader matching tokio::io::BufReader.
pub struct BufReader<R> {
    inner: R,
}

impl<R> BufReader<R> {
    pub fn new(inner: R) -> Self {
        Self { inner }
    }

    pub fn new_with_capacity(_capacity: usize, inner: R) -> Self {
        Self { inner }
    }

    pub fn get_ref(&self) -> &R {
        &self.inner
    }

    pub fn into_inner(self) -> R {
        self.inner
    }

    pub fn take(self, limit: u64) -> Take<Self> {
        Take { inner: self, limit }
    }

    pub fn lines(self) -> Lines<R> {
        Lines { reader: self }
    }

    pub async fn read_until(&mut self, byte: u8, buf: &mut Vec<u8>) -> io::Result<usize>
    where
        R: AsyncRead + Unpin,
    {
        let mut total = 0;
        loop {
            let mut tmp = [0u8; 1];
            let n = AsyncReadExt::read(&mut self.inner, &mut tmp).await?;
            if n == 0 {
                break;
            }
            buf.push(tmp[0]);
            total += 1;
            if tmp[0] == byte {
                break;
            }
        }
        Ok(total)
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for BufReader<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

/// Lines iterator matching tokio::io::Lines.
pub struct Lines<R> {
    reader: BufReader<R>,
}

impl<R: AsyncRead + Unpin> Lines<R> {
    pub async fn next_line(&mut self) -> io::Result<Option<String>> {
        let mut buf = Vec::new();
        let n = self.reader.read_until(b'\n', &mut buf).await?;
        if n == 0 {
            return Ok(None);
        }
        // Strip trailing newline
        if buf.last() == Some(&b'\n') {
            buf.pop();
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
        }
        Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
    }
}

/// BufWriter matching tokio::io::BufWriter.
pub struct BufWriter<W> {
    inner: W,
}

impl<W> BufWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }

    pub fn get_ref(&self) -> &W {
        &self.inner
    }

    pub fn into_inner(self) -> W {
        self.inner
    }
}

/// AsyncBufReadExt (minimal).
pub trait AsyncBufReadExt {
    async fn read_line(&mut self, buf: &mut String) -> io::Result<usize>;
}

/// Duplex stream (stub).
pub fn duplex(max_buf_size: usize) -> (DuplexStream, DuplexStream) {
    (DuplexStream, DuplexStream)
}

pub struct DuplexStream;

/// stdout/stderr/stdin
pub fn stdout() -> Stdout {
    Stdout
}

pub fn stderr() -> Stderr {
    Stderr
}

pub fn stdin() -> Stdin {
    Stdin
}

pub struct Stdout;
pub struct Stderr;
pub struct Stdin;

impl AsyncWrite for Stdout {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for Stderr {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// Take adapter — limits the number of bytes read from the inner reader.
pub struct Take<R> {
    inner: R,
    limit: u64,
}

impl<R> Take<R> {
    pub fn limit(&self) -> u64 {
        self.limit
    }

    pub fn set_limit(&mut self, limit: u64) {
        self.limit = limit;
    }

    pub fn get_ref(&self) -> &R {
        &self.inner
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for Take<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.limit == 0 {
            return Poll::Ready(Ok(()));
        }
        // Limit the read to at most `limit` bytes
        let remaining = this.limit as usize;
        let max = buf.remaining().min(remaining);
        let mut limited_buf_storage = vec![0u8; max];
        let mut limited_buf = ReadBuf::new(&mut limited_buf_storage);
        match Pin::new(&mut this.inner).poll_read(cx, &mut limited_buf) {
            Poll::Ready(Ok(())) => {
                let n = limited_buf.filled;
                buf.put_slice(&limited_buf_storage[..n]);
                this.limit -= n as u64;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}
