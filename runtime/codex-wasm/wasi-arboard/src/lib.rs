#![allow(dead_code, unused_variables)]
//! WASM shim for arboard — clipboard via WIT `host:browser/clipboard` interface.
//!
//! Provides `arboard::Clipboard` by delegating to registered handler functions.
//! The codex-wasm-tui wrapper sets the handlers to call the
//! `host:browser/clipboard@0.1.0` WIT imports before running the TUI.
//! Falls back to in-memory storage if no handlers are registered.

use std::fmt;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Handler function types
// ---------------------------------------------------------------------------

type ReadTextFn = fn() -> Result<String, String>;
type WriteTextFn = fn(&str) -> Result<(), String>;

static READ_HANDLER: Mutex<Option<ReadTextFn>> = Mutex::new(None);
static WRITE_HANDLER: Mutex<Option<WriteTextFn>> = Mutex::new(None);

/// In-memory fallback storage when no WIT handler is registered.
static IN_MEMORY: Mutex<Option<String>> = Mutex::new(None);

/// Register the handler that `Clipboard::get_text()` delegates to.
/// Call this before any code calls clipboard operations.
pub fn set_read_handler(handler: ReadTextFn) {
    *READ_HANDLER.lock().unwrap_or_else(|e| e.into_inner()) = Some(handler);
}

/// Register the handler that `Clipboard::set_text()` delegates to.
/// Call this before any code calls clipboard operations.
pub fn set_write_handler(handler: WriteTextFn) {
    *WRITE_HANDLER.lock().unwrap_or_else(|e| e.into_inner()) = Some(handler);
}

// ---------------------------------------------------------------------------
// Clipboard
// ---------------------------------------------------------------------------

pub struct Clipboard;

impl Clipboard {
    pub fn new() -> Result<Self, Error> {
        Ok(Clipboard)
    }

    pub fn get_text(&mut self) -> Result<String, Error> {
        let handler = READ_HANDLER.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(f) = *handler {
            drop(handler);
            return f().map_err(|e| Error::Unknown(e));
        }
        drop(handler);
        // Fall back to in-memory
        let mem = IN_MEMORY.lock().unwrap_or_else(|e| e.into_inner());
        match mem.as_deref() {
            Some(text) => Ok(text.to_string()),
            None => Err(Error::ContentNotAvailable),
        }
    }

    pub fn set_text(&mut self, text: String) -> Result<(), Error> {
        let handler = WRITE_HANDLER.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(f) = *handler {
            drop(handler);
            // Write to WIT handler and also update in-memory cache
            let result = f(&text).map_err(|e| Error::Unknown(e));
            let mut mem = IN_MEMORY.lock().unwrap_or_else(|e| e.into_inner());
            *mem = Some(text);
            return result;
        }
        drop(handler);
        // Fall back to in-memory
        let mut mem = IN_MEMORY.lock().unwrap_or_else(|e| e.into_inner());
        *mem = Some(text);
        Ok(())
    }

    pub fn get(&mut self) -> Get<'_> {
        Get { _clipboard: self }
    }

    pub fn get_image(&mut self) -> Result<ImageData<'static>, Error> {
        Err(Error::ClipboardNotSupported)
    }
}

pub struct Get<'a> {
    _clipboard: &'a mut Clipboard,
}

impl<'a> Get<'a> {
    pub fn text(self) -> Result<String, Error> {
        self._clipboard.get_text()
    }

    pub fn image(self) -> Result<ImageData<'static>, Error> {
        Err(Error::ClipboardNotSupported)
    }

    pub fn file_list(self) -> Result<Vec<String>, Error> {
        Err(Error::ClipboardNotSupported)
    }
}

#[derive(Clone, Debug)]
pub struct ImageData<'a> {
    pub width: usize,
    pub height: usize,
    pub bytes: std::borrow::Cow<'a, [u8]>,
}

#[derive(Debug)]
pub enum Error {
    ClipboardNotSupported,
    ContentNotAvailable,
    Unknown(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClipboardNotSupported => write!(f, "clipboard not supported in WASM"),
            Self::ContentNotAvailable => write!(f, "content not available"),
            Self::Unknown(s) => write!(f, "unknown error: {s}"),
        }
    }
}

impl std::error::Error for Error {}
