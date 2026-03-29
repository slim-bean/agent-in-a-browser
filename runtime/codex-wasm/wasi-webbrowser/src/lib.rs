//! WASM shim for the `webbrowser` crate.
//!
//! Provides `webbrowser::open(url)` by delegating to a registered handler.
//! The codex-wasm-tui wrapper sets the handler to call the
//! `host:browser/actions@0.1.0#open-url` WIT import before running the TUI.

use std::fmt;
use std::sync::Mutex;

/// Error type matching the real webbrowser crate's error interface.
#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for Error {}

type OpenUrlFn = fn(&str) -> Result<(), String>;

static HANDLER: Mutex<Option<OpenUrlFn>> = Mutex::new(None);

/// Register the handler that `open()` delegates to.
/// Call this before any code calls `webbrowser::open()`.
pub fn set_open_handler(handler: OpenUrlFn) {
    *HANDLER.lock().unwrap_or_else(|e| e.into_inner()) = Some(handler);
}

/// Open a URL in the browser.
///
/// Delegates to the registered handler (typically the WIT `open-url` binding).
pub fn open(url: &str) -> Result<(), Error> {
    let handler = HANDLER.lock().unwrap_or_else(|e| e.into_inner());
    match *handler {
        Some(f) => f(url).map_err(Error),
        None => Err(Error("webbrowser: no open handler registered".into())),
    }
}
