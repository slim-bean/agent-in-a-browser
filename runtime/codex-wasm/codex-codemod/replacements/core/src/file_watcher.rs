//! Stub module — file watcher replaced with no-ops for wasip2.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::config::Config;
use crate::skills::SkillsManager;

/// Events emitted by the file watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileWatcherEvent {
    pub paths: Vec<PathBuf>,
}

/// A path to watch.
#[derive(Debug, Clone)]
pub struct WatchPath {
    pub path: PathBuf,
    pub recursive: bool,
}

/// Receiver end of a file watcher subscriber channel — no-op stub.
pub struct Receiver {
    rx: tokio::sync::mpsc::UnboundedReceiver<FileWatcherEvent>,
}

impl Receiver {
    pub async fn recv(&mut self) -> Option<FileWatcherEvent> {
        self.rx.recv().await
    }
}

/// Throttled wrapper around Receiver — no-op stub.
pub struct ThrottledWatchReceiver {
    rx: Receiver,
    _interval: Duration,
}

impl ThrottledWatchReceiver {
    pub fn new(rx: Receiver, interval: Duration) -> Self {
        Self { rx, _interval: interval }
    }

    pub async fn recv(&mut self) -> Option<FileWatcherEvent> {
        self.rx.recv().await
    }
}

/// Subscriber handle — allows registering watch paths.
pub struct FileWatcherSubscriber {
    _watcher: Arc<FileWatcher>,
}

impl FileWatcherSubscriber {
    pub fn register_paths(&self, _paths: Vec<WatchPath>) -> WatchRegistration {
        WatchRegistration
    }

    pub fn register_path(&self, _path: PathBuf, _recursive: bool) -> WatchRegistration {
        WatchRegistration
    }
}

/// File watcher — no-op in WASM (notify crate not available).
pub struct FileWatcher {
    _tx: tokio::sync::broadcast::Sender<()>,
}

impl FileWatcher {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (_tx, _) = tokio::sync::broadcast::channel(16);
        Ok(Self { _tx })
    }

    pub(crate) fn noop() -> Self {
        let (_tx, _) = tokio::sync::broadcast::channel(16);
        Self { _tx }
    }

    pub fn add_subscriber(self: &Arc<Self>) -> (FileWatcherSubscriber, Receiver) {
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (
            FileWatcherSubscriber { _watcher: Arc::clone(self) },
            Receiver { rx },
        )
    }

    pub(crate) fn register_config(
        self: &Arc<Self>,
        _config: &Config,
        _skills_manager: &SkillsManager,
    ) -> WatchRegistration {
        WatchRegistration
    }

}

/// Watch registration handle — no-op in WASM.
pub struct WatchRegistration;

impl Drop for WatchRegistration {
    fn drop(&mut self) {}
}
