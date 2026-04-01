#![allow(dead_code, unused_variables)]
//! Stub for arboard in wasip2 — clipboard not available.

use std::fmt;

pub struct Clipboard;

static CLIPBOARD_STORE: std::sync::LazyLock<std::sync::Mutex<String>> = 
    std::sync::LazyLock::new(|| std::sync::Mutex::new(String::new()));

impl Clipboard {
    pub fn new() -> Result<Self, Error> {
        Ok(Self)
    }

    pub fn get_text(&mut self) -> Result<String, Error> {
        let store = CLIPBOARD_STORE.lock().unwrap();
        if store.is_empty() {
            Err(Error::ContentNotAvailable)
        } else {
            Ok(store.clone())
        }
    }

    pub fn set_text(&mut self, text: String) -> Result<(), Error> {
        let mut store = CLIPBOARD_STORE.lock().unwrap();
        *store = text;
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
        let store = CLIPBOARD_STORE.lock().unwrap();
        if store.is_empty() {
            Err(Error::ContentNotAvailable)
        } else {
            Ok(store.clone())
        }
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
