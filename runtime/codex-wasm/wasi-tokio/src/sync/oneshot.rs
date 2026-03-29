//! One-shot channel matching tokio::sync::oneshot.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Arc::new(Mutex::new(None));
    (
        Sender {
            inner: inner.clone(),
        },
        Receiver { inner },
    )
}

pub struct Sender<T> {
    inner: Arc<Mutex<Option<T>>>,
}

impl<T> std::fmt::Debug for Sender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sender").finish()
    }
}

impl<T> Sender<T> {
    pub fn send(self, value: T) -> Result<(), T> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        *inner = Some(value);
        Ok(())
    }

    pub fn is_closed(&self) -> bool {
        // In single-threaded WASM, receiver is always alive if we have a ref
        false
    }
}

pub struct Receiver<T> {
    inner: Arc<Mutex<Option<T>>>,
}

impl<T> std::fmt::Debug for Receiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Receiver").finish()
    }
}

impl<T> Future for Receiver<T> {
    type Output = Result<T, RecvError>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match inner.take() {
            Some(val) => Poll::Ready(Ok(val)),
            None => Poll::Pending,
        }
    }
}

impl<T> Receiver<T> {
    pub fn try_recv(&mut self) -> Result<T, TryRecvError> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.take().ok_or(TryRecvError::Empty)
    }
}

#[derive(Debug)]
pub struct RecvError;

impl std::fmt::Display for RecvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "oneshot sender dropped")
    }
}

impl std::error::Error for RecvError {}

#[derive(Debug)]
pub enum TryRecvError {
    Empty,
    Closed,
}

impl std::fmt::Display for TryRecvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "oneshot not ready"),
            Self::Closed => write!(f, "oneshot sender dropped"),
        }
    }
}

impl std::error::Error for TryRecvError {}

/// Error submodule matching tokio::sync::oneshot::error.
pub mod error {
    pub use super::RecvError;
    pub use super::TryRecvError;
}
