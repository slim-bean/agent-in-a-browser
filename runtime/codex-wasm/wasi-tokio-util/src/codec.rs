#![allow(dead_code, unused_variables, unused_imports, unused_mut)]
//! Codec types for framed I/O on async readers/writers.
//!
//! Provides `Decoder`, `Encoder` traits and `FramedRead`/`FramedWrite`
//! adapters that buffer data and apply codecs to produce/consume frames.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::BytesMut;

// ---------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------

pub trait Decoder {
    type Item;
    type Error: From<io::Error>;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error>;

    fn decode_eof(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.decode(src)
    }
}

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

pub trait Encoder<Item> {
    type Error: From<io::Error>;

    fn encode(&mut self, item: Item, dst: &mut BytesMut) -> Result<(), Self::Error>;
}

// ---------------------------------------------------------------------------
// Framed
// ---------------------------------------------------------------------------

/// Pairs an I/O transport with a codec for both reading and writing.
pub struct Framed<T, U> {
    inner: T,
    codec: U,
    read_buf: BytesMut,
    write_buf: BytesMut,
}

impl<T, U> Framed<T, U> {
    pub fn new(inner: T, codec: U) -> Self {
        Framed {
            inner,
            codec,
            read_buf: BytesMut::with_capacity(8192),
            write_buf: BytesMut::with_capacity(8192),
        }
    }

    pub fn get_ref(&self) -> &T {
        &self.inner
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    pub fn into_inner(self) -> T {
        self.inner
    }

    pub fn codec(&self) -> &U {
        &self.codec
    }

    pub fn codec_mut(&mut self) -> &mut U {
        &mut self.codec
    }
}

// ---------------------------------------------------------------------------
// FramedRead
// ---------------------------------------------------------------------------

/// Reads bytes from an `AsyncRead` and decodes them into frames using a `Decoder`.
pub struct FramedRead<T, D> {
    inner: T,
    decoder: D,
    buf: BytesMut,
    eof: bool,
}

impl<T, D> FramedRead<T, D> {
    pub fn new(inner: T, decoder: D) -> Self {
        FramedRead {
            inner,
            decoder,
            buf: BytesMut::with_capacity(8192),
            eof: false,
        }
    }

    pub fn get_ref(&self) -> &T {
        &self.inner
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    pub fn into_inner(self) -> T {
        self.inner
    }

    pub fn decoder(&self) -> &D {
        &self.decoder
    }

    pub fn decoder_mut(&mut self) -> &mut D {
        &mut self.decoder
    }
}

impl<T: tokio::io::AsyncRead + Unpin, D: Decoder + Unpin> futures_core::Stream
    for FramedRead<T, D>
{
    type Item = Result<D::Item, D::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        loop {
            // Try to decode a frame from the buffer first.
            if !this.buf.is_empty() || this.eof {
                if this.eof {
                    // EOF reached — try decode_eof to flush remaining data.
                    match this.decoder.decode_eof(&mut this.buf) {
                        Ok(Some(item)) => return Poll::Ready(Some(Ok(item))),
                        Ok(None) => return Poll::Ready(None),
                        Err(e) => return Poll::Ready(Some(Err(e))),
                    }
                }
                match this.decoder.decode(&mut this.buf) {
                    Ok(Some(item)) => return Poll::Ready(Some(Ok(item))),
                    Ok(None) => {} // Need more data — fall through to read
                    Err(e) => return Poll::Ready(Some(Err(e))),
                }
            }

            // Read more data from the inner reader.
            let mut read_buf = [0u8; 8192];
            let mut tokio_buf = tokio::io::ReadBuf::new(&mut read_buf);
            match Pin::new(&mut this.inner).poll_read(cx, &mut tokio_buf) {
                Poll::Ready(Ok(())) => {
                    let n = tokio_buf.filled().len();
                    if n == 0 {
                        this.eof = true;
                        // Loop back to try decode_eof
                    } else {
                        this.buf.extend_from_slice(&read_buf[..n]);
                        // Loop back to try decoding
                    }
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(e.into()))),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// FramedWrite
// ---------------------------------------------------------------------------

/// Encodes items using an `Encoder` and writes them to an `AsyncWrite`.
pub struct FramedWrite<T, E> {
    inner: T,
    encoder: E,
    buf: BytesMut,
}

impl<T, E> FramedWrite<T, E> {
    pub fn new(inner: T, encoder: E) -> Self {
        FramedWrite {
            inner,
            encoder,
            buf: BytesMut::with_capacity(8192),
        }
    }

    pub fn get_ref(&self) -> &T {
        &self.inner
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    pub fn into_inner(self) -> T {
        self.inner
    }

    pub fn encoder(&self) -> &E {
        &self.encoder
    }

    pub fn encoder_mut(&mut self) -> &mut E {
        &mut self.encoder
    }
}

impl<T: tokio::io::AsyncWrite + Unpin, Item, E: Encoder<Item> + Unpin> futures_sink::Sink<Item>
    for FramedWrite<T, E>
{
    type Error = E::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Flush any buffered data before accepting new items.
        let this = self.get_mut();
        while !this.buf.is_empty() {
            match Pin::new(&mut this.inner).poll_write(cx, &this.buf) {
                Poll::Ready(Ok(n)) => {
                    let _ = this.buf.split_to(n);
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e.into())),
                Poll::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: Item) -> Result<(), Self::Error> {
        let this = self.get_mut();
        this.encoder.encode(item, &mut this.buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        while !this.buf.is_empty() {
            match Pin::new(&mut this.inner).poll_write(cx, &this.buf) {
                Poll::Ready(Ok(n)) => {
                    let _ = this.buf.split_to(n);
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e.into())),
                Poll::Pending => return Poll::Pending,
            }
        }
        Pin::new(&mut this.inner).poll_flush(cx).map_err(Into::into)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        // Flush remaining data first
        while !this.buf.is_empty() {
            match Pin::new(&mut this.inner).poll_write(cx, &this.buf) {
                Poll::Ready(Ok(n)) => {
                    let _ = this.buf.split_to(n);
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e.into())),
                Poll::Pending => return Poll::Pending,
            }
        }
        Pin::new(&mut this.inner)
            .poll_shutdown(cx)
            .map_err(Into::into)
    }
}
