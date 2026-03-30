//! Console logging via WIT `host:console/logging@0.1.0`.
//!
//! Provides `console_log!`, `console_warn!`, and `console_error!` macros
//! that route to the browser console instead of polluting stderr.
//!
//! This crate declares the raw WASM import symbols directly (no wit-bindgen)
//! so any library crate can use it. The linker resolves the symbols when
//! the final cdylib WASM component is built.

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "host:console/logging@0.1.0")]
unsafe extern "C" {
    #[link_name = "log"]
    fn _console_log(ptr: *const u8, len: usize);

    #[link_name = "warn"]
    fn _console_warn(ptr: *const u8, len: usize);

    #[link_name = "error"]
    fn _console_error(ptr: *const u8, len: usize);
}

/// Log a message to the browser console (console.log).
#[inline]
pub fn log(msg: &str) {
    #[cfg(target_arch = "wasm32")]
    unsafe {
        _console_log(msg.as_ptr(), msg.len());
    }
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{msg}");
}

/// Log a warning to the browser console (console.warn).
#[inline]
pub fn warn(msg: &str) {
    #[cfg(target_arch = "wasm32")]
    unsafe {
        _console_warn(msg.as_ptr(), msg.len());
    }
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("[WARN] {msg}");
}

/// Log an error to the browser console (console.error).
#[inline]
pub fn error(msg: &str) {
    #[cfg(target_arch = "wasm32")]
    unsafe {
        _console_error(msg.as_ptr(), msg.len());
    }
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("[ERROR] {msg}");
}

/// Log a formatted message to the browser console (console.log).
#[macro_export]
macro_rules! console_log {
    ($($arg:tt)*) => {
        $crate::log(&format!($($arg)*))
    };
}

/// Log a formatted warning to the browser console (console.warn).
#[macro_export]
macro_rules! console_warn {
    ($($arg:tt)*) => {
        $crate::warn(&format!($($arg)*))
    };
}

/// Log a formatted error to the browser console (console.error).
#[macro_export]
macro_rules! console_error {
    ($($arg:tt)*) => {
        $crate::error(&format!($($arg)*))
    };
}
