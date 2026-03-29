//! Watch channel matching tokio::sync::watch.

use std::sync::{Arc, Mutex};
use std::task::Waker;

pub fn channel<T>(init: T) -> (Sender<T>, Receiver<T>) {
    let inner = Arc::new(Mutex::new(WatchInner {
        value: init,
        version: 0,
        closed: false,
        wakers: Vec::new(),
    }));
    (
        Sender {
            inner: inner.clone(),
        },
        Receiver {
            inner,
            seen_version: 0,
        },
    )
}

struct WatchInner<T> {
    value: T,
    version: u64,
    closed: bool,
    wakers: Vec<Waker>,
}

impl<T> WatchInner<T> {
    fn wake_all(&mut self) {
        for waker in self.wakers.drain(..) {
            waker.wake();
        }
    }
}

pub struct Sender<T> {
    inner: Arc<Mutex<WatchInner<T>>>,
}

impl<T> Sender<T> {
    pub fn send(&self, value: T) -> Result<(), SendError<T>> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.closed {
            return Err(SendError(value));
        }
        inner.value = value;
        inner.version += 1;
        inner.wake_all();
        Ok(())
    }

    pub fn send_replace(&self, value: T) -> T
    where
        T: Default,
    {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let old = std::mem::replace(&mut inner.value, value);
        inner.version += 1;
        inner.wake_all();
        old
    }

    pub fn send_modify<F>(&self, func: F)
    where
        F: FnOnce(&mut T),
    {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        func(&mut inner.value);
        inner.version += 1;
        inner.wake_all();
    }

    pub fn subscribe(&self) -> Receiver<T> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let version = inner.version;
        drop(inner);
        Receiver {
            inner: self.inner.clone(),
            seen_version: version,
        }
    }

    pub fn borrow(&self) -> Ref<'_, T> {
        Ref {
            guard: self.inner.lock().unwrap_or_else(|e| e.into_inner()),
        }
    }

    pub fn is_closed(&self) -> bool {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).closed
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

pub struct Receiver<T> {
    inner: Arc<Mutex<WatchInner<T>>>,
    seen_version: u64,
}

impl<T: Clone> Receiver<T> {
    pub async fn changed(&mut self) -> Result<(), RecvError> {
        std::future::poll_fn(|cx| match self.poll_changed(cx.waker()) {
            Ok(true) => {
                self.seen_version = self.inner.lock().unwrap_or_else(|e| e.into_inner()).version;
                std::task::Poll::Ready(Ok(()))
            }
            Ok(false) => std::task::Poll::Pending,
            Err(e) => std::task::Poll::Ready(Err(e)),
        })
        .await
    }

    pub fn borrow(&self) -> Ref<'_, T> {
        Ref {
            guard: self.inner.lock().unwrap_or_else(|e| e.into_inner()),
        }
    }

    pub fn borrow_and_update(&mut self) -> Ref<'_, T> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        self.seen_version = guard.version;
        Ref { guard }
    }

    pub fn has_changed(&self) -> Result<bool, RecvError> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.closed {
            return Err(RecvError(()));
        }
        Ok(inner.version > self.seen_version)
    }

    /// Check for changes and register a waker if none available.
    pub fn poll_changed(&mut self, waker: &Waker) -> Result<bool, RecvError> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.closed {
            return Err(RecvError(()));
        }
        if inner.version > self.seen_version {
            Ok(true)
        } else {
            inner.wakers.push(waker.clone());
            Ok(false)
        }
    }
}

impl<T> std::fmt::Debug for Receiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Receiver").finish()
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            seen_version: self.seen_version,
        }
    }
}

/// Borrowed reference to the watched value.
pub struct Ref<'a, T> {
    guard: std::sync::MutexGuard<'a, WatchInner<T>>,
}

impl<'a, T> std::ops::Deref for Ref<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.guard.value
    }
}

#[derive(Debug)]
pub struct SendError<T>(pub T);

impl<T> std::fmt::Display for SendError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "watch channel closed")
    }
}

impl<T: std::fmt::Debug> std::error::Error for SendError<T> {}

#[derive(Debug)]
pub struct RecvError(());

impl std::fmt::Display for RecvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "watch channel closed")
    }
}

impl std::error::Error for RecvError {}

pub mod error {
    pub use super::RecvError;
}
