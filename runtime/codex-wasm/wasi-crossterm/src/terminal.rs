//! Terminal control matching crossterm::terminal.

use std::io;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::OnceLock;

/// Cached terminal dimensions, updated by resize events or host queries.
static TERM_COLS: AtomicU16 = AtomicU16::new(80);
static TERM_ROWS: AtomicU16 = AtomicU16::new(24);

/// Optional host-side size query function. When registered, `size()` calls
/// this to get the current dimensions from the host (e.g., WIT terminal:info/size)
/// instead of relying on cached atomics. This avoids needing stdin-injected
/// resize escape sequences.
static SIZE_QUERY: OnceLock<Box<dyn Fn() -> (u16, u16) + Send + Sync>> = OnceLock::new();

/// Register a function that queries the host for the current terminal size.
/// Called once at startup by the component entry point (codex-wasm-tui).
pub fn set_size_query(f: impl Fn() -> (u16, u16) + Send + Sync + 'static) {
    let _ = SIZE_QUERY.set(Box::new(f));
}

/// Update the cached terminal size. Called when a resize escape
/// sequence (CSI 8;rows;cols t) is parsed from stdin, or by set_size_query.
pub fn update_size(cols: u16, rows: u16) {
    TERM_COLS.store(cols, Ordering::Relaxed);
    TERM_ROWS.store(rows, Ordering::Relaxed);
}

/// Query the host for the current terminal size (if a query function is registered).
pub fn query_host_size() -> Option<(u16, u16)> {
    SIZE_QUERY.get().map(|f| f())
}

/// Get the cached column count (without querying the host).
pub fn cached_cols() -> u16 {
    TERM_COLS.load(Ordering::Relaxed)
}

/// Get the cached row count (without querying the host).
pub fn cached_rows() -> u16 {
    TERM_ROWS.load(Ordering::Relaxed)
}

/// Enable raw mode — no-op in browser terminal (always raw).
pub fn enable_raw_mode() -> io::Result<()> {
    Ok(())
}

/// Disable raw mode — no-op in browser terminal.
pub fn disable_raw_mode() -> io::Result<()> {
    Ok(())
}

/// Check if the terminal supports keyboard enhancement — always false in WASM.
pub fn supports_keyboard_enhancement() -> io::Result<bool> {
    Ok(false)
}

/// Get terminal size. Queries the host via the registered size query function
/// if available, otherwise returns cached values.
pub fn size() -> io::Result<(u16, u16)> {
    if let Some(query) = SIZE_QUERY.get() {
        let (cols, rows) = query();
        // Update cache for consistency
        TERM_COLS.store(cols, Ordering::Relaxed);
        TERM_ROWS.store(rows, Ordering::Relaxed);
        Ok((cols, rows))
    } else {
        Ok((
            TERM_COLS.load(Ordering::Relaxed),
            TERM_ROWS.load(Ordering::Relaxed),
        ))
    }
}

/// Terminal window size (extended).
#[derive(Debug, Clone, Copy)]
pub struct WindowSize {
    pub rows: u16,
    pub columns: u16,
    pub width: u16,
    pub height: u16,
}

pub fn window_size() -> io::Result<WindowSize> {
    let (cols, rows) = size()?;
    Ok(WindowSize {
        rows,
        columns: cols,
        width: cols * 8,   // approximate pixel width
        height: rows * 16, // approximate pixel height
    })
}

/// Enter alternate screen.
#[derive(Debug)]
pub struct EnterAlternateScreen;

impl super::Command for EnterAlternateScreen {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[?1049h")
    }
}

/// Leave alternate screen.
#[derive(Debug)]
pub struct LeaveAlternateScreen;

impl super::Command for LeaveAlternateScreen {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[?1049l")
    }
}

/// Clear the terminal.
#[derive(Debug)]
pub enum ClearType {
    All,
    Purge,
    CurrentLine,
    UntilNewLine,
    FromCursorDown,
    FromCursorUp,
}

#[derive(Debug)]
pub struct Clear(pub ClearType);

impl super::Command for Clear {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        match self.0 {
            ClearType::All => write!(f, "\x1b[2J"),
            ClearType::Purge => write!(f, "\x1b[3J"),
            ClearType::CurrentLine => write!(f, "\x1b[2K"),
            ClearType::UntilNewLine => write!(f, "\x1b[K"),
            ClearType::FromCursorDown => write!(f, "\x1b[J"),
            ClearType::FromCursorUp => write!(f, "\x1b[1J"),
        }
    }
}

/// Set terminal title.
#[derive(Debug)]
pub struct SetTitle(pub String);

impl super::Command for SetTitle {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b]0;{}\x07", self.0)
    }
}

/// Scroll up.
#[derive(Debug)]
pub struct ScrollUp(pub u16);

impl super::Command for ScrollUp {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[{}S", self.0)
    }
}

/// Scroll down.
#[derive(Debug)]
pub struct ScrollDown(pub u16);

impl super::Command for ScrollDown {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[{}T", self.0)
    }
}

/// Disable line wrap.
#[derive(Debug)]
pub struct DisableLineWrap;

impl super::Command for DisableLineWrap {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[?7l")
    }
}

/// Enable line wrap.
#[derive(Debug)]
pub struct EnableLineWrap;

impl super::Command for EnableLineWrap {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[?7h")
    }
}
