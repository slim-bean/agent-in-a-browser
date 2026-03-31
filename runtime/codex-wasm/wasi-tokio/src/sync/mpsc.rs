//! Multi-producer single-consumer channel matching tokio::sync::mpsc.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

#[track_caller]
pub fn channel<T: 'static>(buffer: usize) -> (Sender<T>, Receiver<T>) {
    let loc = std::panic::Location::caller();
    let id = crate::diagnostics::NEXT_CHANNEL_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let label = format!(
        "{}:{}",
        loc.file().rsplit('/').next().unwrap_or(loc.file()),
        loc.line()
    );

    let inner = Arc::new(Mutex::new(ChannelInner {
        queue: VecDeque::with_capacity(buffer),
        closed: false,
        wakers: Vec::new(),
    }));

    // Register for diagnostics with a weak reference
    let weak = Arc::downgrade(&inner);
    let cap = Some(buffer);
    let diag_label = label.clone();
    crate::diagnostics::register_channel(Box::new(move || {
        let inner = weak.upgrade()?;
        let guard = inner.lock().ok()?;
        Some(crate::diagnostics::ChannelSnapshot {
            id,
            kind: "mpsc",
            label: diag_label.clone(),
            queue_len: guard.queue.len(),
            capacity: cap,
            closed: guard.closed,
            sender_count: Arc::strong_count(&weak.upgrade()?) - 1,
            receiver_alive: !guard.closed,
            pending_wakers: guard.wakers.len(),
        })
    }));

    (
        Sender {
            inner: inner.clone(),
        },
        Receiver { inner },
    )
}

#[track_caller]
pub fn unbounded_channel<T: 'static>() -> (UnboundedSender<T>, UnboundedReceiver<T>) {
    let loc = std::panic::Location::caller();
    let id = crate::diagnostics::NEXT_CHANNEL_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let label = format!(
        "{}:{}",
        loc.file().rsplit('/').next().unwrap_or(loc.file()),
        loc.line()
    );

    let inner = Arc::new(Mutex::new(ChannelInner {
        queue: VecDeque::new(),
        closed: false,
        wakers: Vec::new(),
    }));

    // Register for diagnostics with a weak reference
    let weak = Arc::downgrade(&inner);
    let diag_label = label.clone();
    crate::diagnostics::register_channel(Box::new(move || {
        let inner = weak.upgrade()?;
        let guard = inner.lock().ok()?;
        Some(crate::diagnostics::ChannelSnapshot {
            id,
            kind: "mpsc-unbounded",
            label: diag_label.clone(),
            queue_len: guard.queue.len(),
            capacity: None,
            closed: guard.closed,
            sender_count: Arc::strong_count(&weak.upgrade()?) - 1,
            receiver_alive: !guard.closed,
            pending_wakers: guard.wakers.len(),
        })
    }));

    (
        UnboundedSender {
            inner: inner.clone(),
        },
        UnboundedReceiver { inner },
    )
}

struct ChannelInner<T> {
    queue: VecDeque<T>,
    closed: bool,
    wakers: Vec<Waker>,
}

impl<T> ChannelInner<T> {
    fn wake_all(&mut self) {
        for waker in self.wakers.drain(..) {
            waker.wake();
        }
    }
}

// -- Bounded --

pub struct Sender<T> {
    inner: Arc<Mutex<ChannelInner<T>>>,
}

impl<T> std::fmt::Debug for Sender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sender").finish()
    }
}

// Manual Clone — doesn't require T: Clone (matches real tokio)
impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> Sender<T> {
    pub async fn send(&self, value: T) -> Result<(), SendError<T>> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.closed {
            return Err(SendError(value));
        }
        inner.queue.push_back(value);
        inner.wake_all();
        Ok(())
    }

    pub fn try_send(&self, value: T) -> Result<(), TrySendError<T>> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.closed {
            return Err(TrySendError::Closed(value));
        }
        inner.queue.push_back(value);
        inner.wake_all();
        Ok(())
    }

    pub fn is_closed(&self) -> bool {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).closed
    }

    pub fn blocking_send(&self, value: T) -> Result<(), SendError<T>> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.closed {
            return Err(SendError(value));
        }
        inner.queue.push_back(value);
        inner.wake_all();
        Ok(())
    }
}

pub struct Receiver<T> {
    inner: Arc<Mutex<ChannelInner<T>>>,
}

impl<T> std::fmt::Debug for Receiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Receiver").finish()
    }
}

impl<T> Receiver<T> {
    pub async fn recv(&mut self) -> Option<T> {
        std::future::poll_fn(|cx| self.poll_recv(cx)).await
    }

    /// Poll-based receive — used by Stream impls.
    pub fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match inner.queue.pop_front() {
            Some(val) => Poll::Ready(Some(val)),
            None if inner.closed => Poll::Ready(None),
            None => {
                inner.wakers.push(cx.waker().clone());
                Poll::Pending
            }
        }
    }

    pub fn try_recv(&mut self) -> Result<T, TryRecvError> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.queue.pop_front().ok_or(TryRecvError::Empty)
    }

    pub fn close(&mut self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.closed = true;
    }

    pub fn is_closed(&self) -> bool {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).closed
    }

    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .queue
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn capacity(&self) -> usize {
        1024
    }

    pub fn max_capacity(&self) -> usize {
        1024
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.close();
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        // If this is the last sender (only receiver's Arc remains after we drop),
        // close the channel so recv() returns None.
        // Weak refs from diagnostics don't affect strong_count.
        if Arc::strong_count(&self.inner) == 2 {
            if let Ok(mut inner) = self.inner.lock() {
                inner.closed = true;
                inner.wake_all();
            }
        }
    }
}

// -- Unbounded --

pub struct UnboundedSender<T> {
    inner: Arc<Mutex<ChannelInner<T>>>,
}

impl<T> std::fmt::Debug for UnboundedSender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnboundedSender").finish()
    }
}

// Manual Clone — doesn't require T: Clone (matches real tokio)
impl<T> Clone for UnboundedSender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> UnboundedSender<T> {
    pub fn send(&self, value: T) -> Result<(), SendError<T>> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.closed {
            return Err(SendError(value));
        }
        inner.queue.push_back(value);
        inner.wake_all();
        Ok(())
    }

    pub fn is_closed(&self) -> bool {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).closed
    }
}

impl<T> Drop for UnboundedSender<T> {
    fn drop(&mut self) {
        // If this is the last sender (only receiver's Arc remains after we drop),
        // close the channel so recv() returns None.
        // Weak refs from diagnostics don't affect strong_count.
        if Arc::strong_count(&self.inner) == 2 {
            if let Ok(mut inner) = self.inner.lock() {
                inner.closed = true;
                inner.wake_all();
            }
        }
    }
}

pub struct UnboundedReceiver<T> {
    inner: Arc<Mutex<ChannelInner<T>>>,
}

impl<T> UnboundedReceiver<T> {
    pub async fn recv(&mut self) -> Option<T> {
        std::future::poll_fn(|cx| {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            match inner.queue.pop_front() {
                Some(val) => Poll::Ready(Some(val)),
                None if inner.closed => Poll::Ready(None),
                None => {
                    inner.wakers.push(cx.waker().clone());
                    Poll::Pending
                }
            }
        })
        .await
    }

    pub fn try_recv(&mut self) -> Result<T, TryRecvError> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.queue.pop_front().ok_or(TryRecvError::Empty)
    }

    pub fn close(&mut self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.closed = true;
    }

    pub fn is_closed(&self) -> bool {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).closed
    }

    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .queue
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn poll_recv(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Option<T>> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match inner.queue.pop_front() {
            Some(val) => Poll::Ready(Some(val)),
            None if inner.closed => Poll::Ready(None),
            None => {
                inner.wakers.push(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl<T> std::fmt::Debug for UnboundedReceiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnboundedReceiver").finish()
    }
}

impl<T> Drop for UnboundedReceiver<T> {
    fn drop(&mut self) {
        self.close();
    }
}

impl<T> futures_core::Stream for UnboundedReceiver<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match inner.queue.pop_front() {
            Some(val) => Poll::Ready(Some(val)),
            None if inner.closed => Poll::Ready(None),
            None => {
                inner.wakers.push(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

// -- Errors --

#[derive(Debug)]
pub struct SendError<T>(pub T);

impl<T> std::fmt::Display for SendError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "channel closed")
    }
}

impl<T: std::fmt::Debug> std::error::Error for SendError<T> {}

#[derive(Debug)]
pub enum TrySendError<T> {
    Full(T),
    Closed(T),
}

impl<T> std::fmt::Display for TrySendError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full(_) => write!(f, "channel full"),
            Self::Closed(_) => write!(f, "channel closed"),
        }
    }
}

impl<T: std::fmt::Debug> std::error::Error for TrySendError<T> {}

#[derive(Debug, PartialEq, Eq)]
pub enum TryRecvError {
    Empty,
    Disconnected,
}

pub mod error {
    pub use super::SendError;
    pub use super::TryRecvError;
    pub use super::TrySendError;
}
