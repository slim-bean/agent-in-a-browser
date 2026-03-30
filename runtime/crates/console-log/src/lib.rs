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

// ---------------------------------------------------------------------------
// std::io::Write adapter for tracing_subscriber
// ---------------------------------------------------------------------------

/// A writer that buffers a single log line and flushes it to console.log.
/// Implements `std::io::Write` so it can be used with `tracing_subscriber::fmt`.
pub struct ConsoleWriter {
    buf: Vec<u8>,
}

impl std::io::Write for ConsoleWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if !self.buf.is_empty() {
            let msg = String::from_utf8_lossy(&self.buf);
            let trimmed = msg.trim_end_matches('\n');
            if !trimmed.is_empty() {
                // Route to appropriate console level based on tracing level prefix
                if trimmed.contains(" ERROR ") {
                    error(trimmed);
                } else if trimmed.contains(" WARN ") {
                    warn(trimmed);
                } else {
                    log(trimmed);
                }
            }
            self.buf.clear();
        }
        Ok(())
    }
}

impl Drop for ConsoleWriter {
    fn drop(&mut self) {
        let _ = std::io::Write::flush(self);
    }
}

/// Factory that creates `ConsoleWriter` instances.
/// Use with `tracing_subscriber::fmt::layer().with_writer(console_log::MakeConsoleWriter)`.
pub struct MakeConsoleWriter;

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for MakeConsoleWriter {
    type Writer = ConsoleWriter;

    fn make_writer(&'a self) -> Self::Writer {
        ConsoleWriter {
            buf: Vec::with_capacity(256),
        }
    }
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
