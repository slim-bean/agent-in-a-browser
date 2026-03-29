#![allow(dead_code, unused_variables, unexpected_cfgs)]
//! wasi-crossterm: A crossterm-compatible API shim for wasip2 environments.
//!
//! Maps terminal events and rendering to the browser's xterm.js terminal
//! via WIT interfaces. The browser side pushes terminal events through
//! an imported WIT function, and we write output to the terminal's
//! write buffer.

/// Macro that prepends the CSI escape sequence (`\x1b[`) to its arguments.
#[macro_export]
macro_rules! csi {
    ($($arg:tt)*) => { concat!("\x1b[", $($arg)*) };
}

pub mod cursor;
pub mod event;
pub mod execute;
pub mod style;
pub mod terminal;

use std::io;

/// Execute a crossterm command on the given writer.
/// In WASM, this writes the ANSI escape sequence to the output buffer.
///
/// Uses `std::fmt::Write` to match the real crossterm `Command` trait signature,
/// which ratatui depends on.
pub trait Command {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result;

    /// ExecutableCommand support.
    #[cfg(feature = "ansi-support")]
    fn execute_ansi(&self) -> io::Result<()> {
        Ok(())
    }
}

/// QueueableCommand — queue a command for later execution.
pub trait QueueableCommand {
    fn queue(&mut self, command: impl Command) -> io::Result<&mut Self>;
}

impl<W: io::Write> QueueableCommand for W {
    fn queue(&mut self, command: impl Command) -> io::Result<&mut Self> {
        let mut buf = String::new();
        command
            .write_ansi(&mut buf)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        self.write_all(buf.as_bytes())?;
        Ok(self)
    }
}

/// ExecutableCommand — execute a command immediately.
pub trait ExecutableCommand {
    fn execute(&mut self, command: impl Command) -> io::Result<&mut Self>;
}

impl<W: io::Write> ExecutableCommand for W {
    fn execute(&mut self, command: impl Command) -> io::Result<&mut Self> {
        let mut buf = String::new();
        command
            .write_ansi(&mut buf)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        self.write_all(buf.as_bytes())?;
        self.flush()?;
        Ok(self)
    }
}

/// Trait providing sync_update() method on writers — no-op in WASM.
pub trait SynchronizedUpdate: Sized {
    fn sync_update<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        f(self)
    }
}

impl<W: io::Write> SynchronizedUpdate for W {}

/// Synchronized update — no-op command in WASM.
///
/// In real crossterm this wraps terminal output in DCS sequences for
/// synchronized rendering. In WASM/browser this is unnecessary.
#[derive(Debug)]
pub struct SynchronizedUpdateCommand;

impl Command for SynchronizedUpdateCommand {
    fn write_ansi(&self, _f: &mut impl std::fmt::Write) -> std::fmt::Result {
        Ok(())
    }
}
