#![allow(dead_code, unused_variables, unused_imports, unused_mut)]
//! Minimal codec stubs for wasip2 environments.
//!
//! Provides the `Decoder`, `Encoder` traits and the `Framed` adapter that
//! `rmcp` (and other crates) import via `tokio_util::codec::*`.

use std::io;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

// ---------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------

pub trait Decoder {
    type Item;
    type Error: From<io::Error>;

    fn decode(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error>;

    fn decode_eof(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.decode(src)
    }
}

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

pub trait Encoder<Item> {
    type Error: From<io::Error>;

    fn encode(&mut self, item: Item, dst: &mut bytes::BytesMut) -> Result<(), Self::Error>;
}

// ---------------------------------------------------------------------------
// Framed
// ---------------------------------------------------------------------------

/// A unified adapter that pairs an I/O transport with a codec.
///
/// This is a stub: it stores the inner I/O object and codec but does not
/// implement actual framed reading/writing. It exists so that code which
/// *constructs* a `Framed` compiles inside the WASM shim environment.
pub struct Framed<T, U> {
    inner: T,
    codec: U,
}

impl<T, U> Framed<T, U> {
    pub fn new(inner: T, codec: U) -> Self {
        Framed { inner, codec }
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
// FramedRead / FramedWrite (commonly imported alongside Framed)
// ---------------------------------------------------------------------------

pub struct FramedRead<T, D> {
    inner: T,
    decoder: D,
}

impl<T, D> FramedRead<T, D> {
    pub fn new(inner: T, decoder: D) -> Self {
        FramedRead { inner, decoder }
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

impl<T, D: Decoder> futures_core::Stream for FramedRead<T, D> {
    type Item = Result<D::Item, D::Error>;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Stub: WASM doesn't do async stream reading on raw I/O
        Poll::Pending
    }
}

pub struct FramedWrite<T, E> {
    inner: T,
    encoder: E,
}

impl<T, E> FramedWrite<T, E> {
    pub fn new(inner: T, encoder: E) -> Self {
        FramedWrite { inner, encoder }
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

impl<T, Item, E: Encoder<Item>> futures_sink::Sink<Item> for FramedWrite<T, E> {
    type Error = E::Error;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, _item: Item) -> Result<(), Self::Error> {
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}
