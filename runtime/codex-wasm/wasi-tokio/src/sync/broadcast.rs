//! Broadcast channel matching tokio::sync::broadcast.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::task::Waker;

pub fn channel<T: Clone>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let inner = Arc::new(Mutex::new(BroadcastInner {
        buffer: VecDeque::with_capacity(capacity),
        capacity,
        next_id: 0,
        closed: false,
        wakers: Vec::new(),
    }));
    let sender = Sender {
        inner: inner.clone(),
    };
    let receiver = Receiver { inner, read_id: 0 };
    (sender, receiver)
}

struct BroadcastInner<T> {
    buffer: VecDeque<(u64, T)>,
    capacity: usize,
    next_id: u64,
    closed: bool,
    wakers: Vec<Waker>,
}

#[derive(Clone)]
pub struct Sender<T: Clone> {
    inner: Arc<Mutex<BroadcastInner<T>>>,
}

impl<T: Clone> Sender<T> {
    pub fn send(&self, value: T) -> Result<usize, SendError<T>> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.closed {
            return Err(SendError(value));
        }
        let id = inner.next_id;
        inner.next_id += 1;
        inner.buffer.push_back((id, value));
        // Trim to capacity
        while inner.buffer.len() > inner.capacity {
            inner.buffer.pop_front();
        }
        // Wake all pending receivers
        for waker in inner.wakers.drain(..) {
            waker.wake();
        }
        Ok(1) // receiver count not tracked precisely
    }

    pub fn subscribe(&self) -> Receiver<T> {
        let inner_guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let read_id = inner_guard.next_id;
        drop(inner_guard);
        Receiver {
            inner: self.inner.clone(),
            read_id,
        }
    }

    pub fn receiver_count(&self) -> usize {
        // Not precisely tracked in this simple implementation
        1
    }
}

pub struct Receiver<T> {
    inner: Arc<Mutex<BroadcastInner<T>>>,
    read_id: u64,
}

impl<T> std::fmt::Debug for Receiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Receiver").finish()
    }
}

impl<T: Clone> Receiver<T> {
    pub fn resubscribe(&self) -> Self {
        let inner_guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let read_id = inner_guard.next_id;
        drop(inner_guard);
        Receiver {
            inner: self.inner.clone(),
            read_id,
        }
    }

    pub fn try_recv(&mut self) -> Result<T, TryRecvError> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for (id, val) in &inner.buffer {
            if *id >= self.read_id {
                self.read_id = id + 1;
                return Ok(val.clone());
            }
        }
        if inner.closed {
            Err(TryRecvError::Closed)
        } else {
            Err(TryRecvError::Empty)
        }
    }

    /// try_recv that also registers a waker for when new data arrives.
    pub fn poll_recv(&mut self, waker: &Waker) -> Result<T, TryRecvError> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for (id, val) in &inner.buffer {
            if *id >= self.read_id {
                self.read_id = id + 1;
                return Ok(val.clone());
            }
        }
        if inner.closed {
            Err(TryRecvError::Closed)
        } else {
            // Register waker so send() can wake us
            inner.wakers.push(waker.clone());
            Err(TryRecvError::Empty)
        }
    }

    pub async fn recv(&mut self) -> Result<T, RecvError> {
        std::future::poll_fn(|cx| match self.poll_recv(cx.waker()) {
            Ok(val) => std::task::Poll::Ready(Ok(val)),
            Err(TryRecvError::Lagged(n)) => std::task::Poll::Ready(Err(RecvError::Lagged(n))),
            Err(TryRecvError::Closed) => std::task::Poll::Ready(Err(RecvError::Closed)),
            Err(TryRecvError::Empty) => std::task::Poll::Pending,
        })
        .await
    }
}

impl<T: Clone> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            read_id: self.read_id,
        }
    }
}

#[derive(Debug)]
pub struct SendError<T>(pub T);

impl<T> std::fmt::Display for SendError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "broadcast channel closed")
    }
}

impl<T: std::fmt::Debug> std::error::Error for SendError<T> {}

#[derive(Debug)]
pub enum RecvError {
    Closed,
    Lagged(u64),
}

#[derive(Debug)]
pub enum TryRecvError {
    Empty,
    Closed,
    Lagged(u64),
}

impl std::fmt::Display for RecvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "broadcast channel closed"),
            Self::Lagged(n) => write!(f, "lagged by {n} messages"),
        }
    }
}

impl std::error::Error for RecvError {}

/// Error submodule matching tokio::sync::broadcast::error.
pub mod error {
    pub use super::RecvError;
    pub use super::SendError;
    pub use super::TryRecvError;
}
