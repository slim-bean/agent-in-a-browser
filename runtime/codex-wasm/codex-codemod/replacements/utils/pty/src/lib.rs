//! Stub — codex-utils-pty for wasip2.
#![allow(dead_code, unused_variables, unused_imports)]
use std::io;
use std::process::Child;
use tokio::sync::mpsc;

pub const DEFAULT_OUTPUT_BYTES_CAP: usize = 1024 * 1024;

pub fn conpty_supported() -> bool { false }

pub mod process_group {
    use std::io;
    use tokio::process::Child;

    pub fn kill_child_process_group(_child: &mut Child) -> io::Result<()> { Ok(()) }
    pub fn kill_process_group_by_pid(_pid: u32) -> io::Result<()> { Ok(()) }
    pub fn terminate_process_group(_pgid: u32) -> io::Result<bool> { Ok(false) }
    pub fn kill_process_group(_pgid: u32) -> io::Result<()> { Ok(()) }
    pub fn detach_from_tty() -> io::Result<()> { Ok(()) }
    pub fn set_parent_death_signal(_parent_pid: i32) -> io::Result<()> { Ok(()) }
    pub fn set_process_group() -> io::Result<()> { Ok(()) }
}

pub mod pty {
    use super::*;
    pub fn conpty_supported() -> bool { false }
    #[allow(clippy::too_many_arguments)]
    pub async fn spawn_process_with_inherited_fds(
        _program: &str, _args: &[String], _cwd: &std::path::Path,
        _env: &std::collections::HashMap<String, String>,
        _arg0: &Option<String>,
        _size: TerminalSize,
        _inherited_fds: &[i32],
    ) -> anyhow::Result<SpawnedProcess> {
        anyhow::bail!("PTY not available in WASM")
    }
}

pub mod pipe {
    use super::*;
    pub async fn spawn_process_no_stdin_with_inherited_fds(
        _program: &str, _args: &[String], _cwd: &std::path::Path,
        _env: &std::collections::HashMap<String, String>,
        _arg0: &Option<String>,
        _inherited_fds: &[i32],
    ) -> anyhow::Result<SpawnedProcess> {
        anyhow::bail!("pipe spawn not available in WASM")
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TerminalSize { pub rows: u16, pub cols: u16 }
impl Default for TerminalSize { fn default() -> Self { Self { rows: 24, cols: 80 } } }

pub struct PtyHandles;

#[derive(Debug)]
pub struct ProcessHandle {
    _private: (),
}

impl ProcessHandle {
    pub fn writer_sender(&self) -> mpsc::Sender<Vec<u8>> { let (tx, _rx) = mpsc::channel(1); tx }
    pub fn has_exited(&self) -> bool { true }
    pub fn exit_code(&self) -> Option<i32> { Some(1) }
    pub fn resize(&self, _size: TerminalSize) -> anyhow::Result<()> { Ok(()) }
    pub fn close_stdin(&self) {}
    pub fn request_terminate(&self) {}
    pub fn terminate(&self) {}
}

pub type ExecCommandSession = ProcessHandle;

pub struct SpawnedProcess {
    pub session: ProcessHandle,
    pub stdout_rx: tokio::sync::broadcast::Receiver<Vec<u8>>,
    pub stderr_rx: tokio::sync::broadcast::Receiver<Vec<u8>>,
    pub exit_rx: tokio::sync::oneshot::Receiver<Option<i32>>,
}

pub type SpawnedPty = SpawnedProcess;

pub fn combine_output_receivers(
    stdout_rx: tokio::sync::broadcast::Receiver<Vec<u8>>,
    _stderr_rx: tokio::sync::broadcast::Receiver<Vec<u8>>,
) -> tokio::sync::broadcast::Receiver<Vec<u8>> {
    stdout_rx
}
