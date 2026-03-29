//! Multi-producer single-consumer channel matching tokio::sync::mpsc.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

pub fn channel<T>(buffer: usize) -> (Sender<T>, Receiver<T>) {
    let inner = Arc::new(Mutex::new(ChannelInner {
        queue: VecDeque::with_capacity(buffer),
        closed: false,
        wakers: Vec::new(),
    }));
    (
        Sender {
            inner: inner.clone(),
        },
        Receiver { inner },
    )
}

pub fn unbounded_channel<T>() -> (UnboundedSender<T>, UnboundedReceiver<T>) {
    let inner = Arc::new(Mutex::new(ChannelInner {
        queue: VecDeque::new(),
        closed: false,
        wakers: Vec::new(),
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
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.close();
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
