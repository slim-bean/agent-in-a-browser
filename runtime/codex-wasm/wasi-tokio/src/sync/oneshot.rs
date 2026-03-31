//! One-shot channel matching tokio::sync::oneshot.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

#[track_caller]
pub fn channel<T: 'static>() -> (Sender<T>, Receiver<T>) {
    let loc = std::panic::Location::caller();
    let id = crate::diagnostics::NEXT_CHANNEL_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let label = format!(
        "{}:{}",
        loc.file().rsplit('/').next().unwrap_or(loc.file()),
        loc.line()
    );

    let inner = Arc::new(Mutex::new(OneshotInner {
        value: None,
        waker: None,
    }));
    let closed = Arc::new(AtomicBool::new(false));

    // Register for diagnostics with a weak reference
    let weak = Arc::downgrade(&inner);
    let diag_closed = closed.clone();
    let diag_label = label.clone();
    crate::diagnostics::register_channel(Box::new(move || {
        let inner = weak.upgrade()?;
        let guard = inner.lock().ok()?;
        let is_closed = diag_closed.load(Ordering::Relaxed);
        Some(crate::diagnostics::ChannelSnapshot {
            id,
            kind: "oneshot",
            label: diag_label.clone(),
            queue_len: if guard.value.is_some() { 1 } else { 0 },
            capacity: Some(1),
            closed: is_closed,
            sender_count: Arc::strong_count(&weak.upgrade()?) - 1,
            receiver_alive: !is_closed,
            pending_wakers: if guard.waker.is_some() { 1 } else { 0 },
        })
    }));

    (
        Sender {
            inner: inner.clone(),
            closed: closed.clone(),
        },
        Receiver { inner, closed },
    )
}

struct OneshotInner<T> {
    value: Option<T>,
    waker: Option<Waker>,
}

pub struct Sender<T> {
    inner: Arc<Mutex<OneshotInner<T>>>,
    closed: Arc<AtomicBool>,
}

impl<T> std::fmt::Debug for Sender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sender").finish()
    }
}

impl<T> Sender<T> {
    pub fn send(self, value: T) -> Result<(), T> {
        if self.closed.load(Ordering::Acquire) {
            return Err(value);
        }
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.value = Some(value);
        if let Some(waker) = inner.waker.take() {
            waker.wake();
        }
        Ok(())
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        // Signal that the sender is gone so the receiver returns RecvError.
        self.closed.store(true, Ordering::Release);
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(waker) = inner.waker.take() {
                waker.wake();
            }
        }
    }
}

pub struct Receiver<T> {
    inner: Arc<Mutex<OneshotInner<T>>>,
    closed: Arc<AtomicBool>,
}

impl<T> std::fmt::Debug for Receiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Receiver").finish()
    }
}

impl<T> Future for Receiver<T> {
    type Output = Result<T, RecvError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match inner.value.take() {
            Some(val) => Poll::Ready(Ok(val)),
            None if self.closed.load(Ordering::Acquire) => Poll::Ready(Err(RecvError)),
            None => {
                inner.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl<T> Receiver<T> {
    pub fn try_recv(&mut self) -> Result<T, TryRecvError> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match inner.value.take() {
            Some(val) => Ok(val),
            None if self.closed.load(Ordering::Acquire) => Err(TryRecvError::Closed),
            None => Err(TryRecvError::Empty),
        }
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
