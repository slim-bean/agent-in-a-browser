//! Process spawning matching tokio::process.
//!
//! In wasip2, process execution routes through the pluggable process backend
//! which dispatches to the WIT shell-exec interface in the browser sandbox.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

use crate::process_backend;

// ---------------------------------------------------------------------------
// ExitStatus — our own implementation since std::process::ExitStatus is
// zero-sized on wasm32 and can't be constructed.
// ---------------------------------------------------------------------------

/// Process exit status, matching the std::process::ExitStatus API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitStatus {
    code: i32,
}

impl ExitStatus {
    /// Create from a raw exit code.
    pub fn from_raw(code: i32) -> Self {
        Self { code }
    }

    /// Returns true if the process exited successfully (code 0).
    pub fn success(&self) -> bool {
        self.code == 0
    }

    /// Returns the exit code if the process exited normally.
    pub fn code(&self) -> Option<i32> {
        Some(self.code)
    }

    /// Returns the signal that terminated the process, if any.
    /// In WASM, processes are never signal-terminated, so always returns None.
    pub fn signal(&self) -> Option<i32> {
        None
    }
}

impl std::fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "exit status: {}", self.code)
    }
}

/// Process output, matching std::process::Output.
#[derive(Debug)]
pub struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Command builder matching tokio::process::Command.
pub struct Command {
    program: String,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    env: HashMap<String, String>,
    env_clear: bool,
    stdin_cfg: Stdio,
    stdout_cfg: Stdio,
    stderr_cfg: Stdio,
    stdin_data: Option<Vec<u8>>,
    timeout_ms: Option<u32>,
}

impl Command {
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            program: program.as_ref().to_string_lossy().into_owned(),
            args: Vec::new(),
            cwd: None,
            env: HashMap::new(),
            env_clear: false,
            stdin_cfg: Stdio::Null,
            stdout_cfg: Stdio::Inherit,
            stderr_cfg: Stdio::Inherit,
            stdin_data: None,
            timeout_ms: None,
        }
    }

    pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Self {
        self.args.push(arg.as_ref().to_string_lossy().into_owned());
        self
    }

    pub fn args(&mut self, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> &mut Self {
        for arg in args {
            self.arg(arg);
        }
        self
    }

    pub fn current_dir(&mut self, dir: impl AsRef<Path>) -> &mut Self {
        self.cwd = Some(dir.as_ref().to_path_buf());
        self
    }

    pub fn env(&mut self, key: impl AsRef<OsStr>, val: impl AsRef<OsStr>) -> &mut Self {
        self.env.insert(
            key.as_ref().to_string_lossy().into_owned(),
            val.as_ref().to_string_lossy().into_owned(),
        );
        self
    }

    pub fn envs(
        &mut self,
        vars: impl IntoIterator<Item = (impl AsRef<OsStr>, impl AsRef<OsStr>)>,
    ) -> &mut Self {
        for (k, v) in vars {
            self.env(k, v);
        }
        self
    }

    pub fn env_clear(&mut self) -> &mut Self {
        self.env_clear = true;
        self.env.clear();
        self
    }

    pub fn stdin(&mut self, cfg: impl Into<Stdio>) -> &mut Self {
        self.stdin_cfg = cfg.into();
        self
    }

    pub fn stdout(&mut self, cfg: impl Into<Stdio>) -> &mut Self {
        self.stdout_cfg = cfg.into();
        self
    }

    pub fn stderr(&mut self, cfg: impl Into<Stdio>) -> &mut Self {
        self.stderr_cfg = cfg.into();
        self
    }

    pub unsafe fn pre_exec<F: FnMut() -> io::Result<()> + Send + Sync + 'static>(
        &mut self,
        _f: F,
    ) -> &mut Self {
        self // No-op in WASM
    }

    pub fn arg0(&mut self, _arg: impl AsRef<OsStr>) -> &mut Self {
        self // No-op in WASM
    }

    pub fn kill_on_drop(&mut self, _kill: bool) -> &mut Self {
        self // No-op in WASM — processes are managed by the host
    }

    pub fn process_group(&mut self, _pgroup: i32) -> &mut Self {
        self // No-op in WASM
    }

    pub fn uid(&mut self, _id: u32) -> &mut Self {
        self
    }

    pub fn gid(&mut self, _id: u32) -> &mut Self {
        self
    }

    /// Build the ExecRequest from current state.
    fn build_request(&self) -> process_backend::ExecRequest {
        let cwd = self
            .cwd
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".to_string());

        let env: Vec<(String, String)> = self
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        process_backend::ExecRequest {
            program: self.program.clone(),
            args: self.args.clone(),
            cwd,
            env,
            stdin: self.stdin_data.clone(),
            timeout_ms: self.timeout_ms,
        }
    }

    /// Spawn the command, returning a Child with captured stdout/stderr.
    /// Routes through the process backend (WIT shell-exec in browser).
    pub fn spawn(&mut self) -> io::Result<Child> {
        let request = self.build_request();
        let response = process_backend::execute(request)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Ok(Child {
            exit_code: response.exit_code,
            stdout: if matches!(self.stdout_cfg, Stdio::Piped) {
                Some(ChildStdout {
                    data: response.stdout,
                    pos: 0,
                })
            } else {
                None
            },
            stderr: if matches!(self.stderr_cfg, Stdio::Piped) {
                Some(ChildStderr {
                    data: response.stderr,
                    pos: 0,
                })
            } else {
                None
            },
            stdin: if matches!(self.stdin_cfg, Stdio::Piped) {
                Some(ChildStdin { buf: Vec::new() })
            } else {
                None
            },
            done: true,
        })
    }

    /// Execute and capture output (convenience method).
    pub async fn output(&mut self) -> io::Result<Output> {
        // Ensure pipes are set up for capture
        self.stdout_cfg = Stdio::Piped;
        self.stderr_cfg = Stdio::Piped;

        let request = self.build_request();
        let response = process_backend::execute(request)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Ok(Output {
            status: ExitStatus::from_raw(response.exit_code),
            stdout: response.stdout,
            stderr: response.stderr,
        })
    }

    /// Execute and return just the exit status.
    pub async fn status(&mut self) -> io::Result<ExitStatus> {
        let request = self.build_request();
        let response = process_backend::execute(request)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Ok(ExitStatus::from_raw(response.exit_code))
    }
}

/// Child process handle matching tokio::process::Child.
///
/// In the WASM model, the process has already completed by the time
/// spawn() returns (synchronous execution via the host). The Child
/// holds the captured output for reading.
pub struct Child {
    pub stdin: Option<ChildStdin>,
    pub stdout: Option<ChildStdout>,
    pub stderr: Option<ChildStderr>,
    exit_code: i32,
    done: bool,
}

impl Child {
    pub fn id(&self) -> Option<u32> {
        None // No OS PID in WASM
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if self.done {
            Ok(Some(ExitStatus::from_raw(self.exit_code)))
        } else {
            Ok(None)
        }
    }

    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        // Already completed
        Ok(ExitStatus::from_raw(self.exit_code))
    }

    pub async fn wait_with_output(self) -> io::Result<Output> {
        let stdout = self.stdout.map(|s| s.data).unwrap_or_default();
        let stderr = self.stderr.map(|s| s.data).unwrap_or_default();

        Ok(Output {
            status: ExitStatus::from_raw(self.exit_code),
            stdout,
            stderr,
        })
    }

    pub fn start_kill(&mut self) -> io::Result<()> {
        Ok(()) // Already completed
    }

    pub async fn kill(&mut self) -> io::Result<()> {
        Ok(()) // Already completed
    }
}

/// Child stdin — buffers data for the next execution (not streaming in WASM).
pub struct ChildStdin {
    buf: Vec<u8>,
}

impl ChildStdin {
    pub async fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        self.buf.extend_from_slice(data);
        Ok(())
    }
}

/// Child stdout — reads from captured output buffer.
/// Implements AsyncRead so it works with BufReader in the exec pipeline.
pub struct ChildStdout {
    data: Vec<u8>,
    pos: usize,
}

impl ChildStdout {
    pub async fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        let remaining = &self.data[self.pos..];
        let len = remaining.len();
        buf.extend_from_slice(remaining);
        self.pos = self.data.len();
        Ok(len)
    }
}

impl crate::io::AsyncRead for ChildStdout {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &mut crate::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let remaining = &self.data[self.pos..];
        if remaining.is_empty() {
            return std::task::Poll::Ready(Ok(()));
        }
        let to_copy = remaining.len().min(buf.remaining());
        buf.put_slice(&remaining[..to_copy]);
        self.pos += to_copy;
        std::task::Poll::Ready(Ok(()))
    }
}

/// Child stderr — reads from captured output buffer.
pub struct ChildStderr {
    data: Vec<u8>,
    pos: usize,
}

impl ChildStderr {
    pub async fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        let remaining = &self.data[self.pos..];
        let len = remaining.len();
        buf.extend_from_slice(remaining);
        self.pos = self.data.len();
        Ok(len)
    }
}

impl crate::io::AsyncRead for ChildStderr {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &mut crate::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let remaining = &self.data[self.pos..];
        if remaining.is_empty() {
            return std::task::Poll::Ready(Ok(()));
        }
        let to_copy = remaining.len().min(buf.remaining());
        buf.put_slice(&remaining[..to_copy]);
        self.pos += to_copy;
        std::task::Poll::Ready(Ok(()))
    }
}

/// Stdio configuration matching std::process::Stdio.
#[derive(Debug, Clone, Copy)]
pub enum Stdio {
    Inherit,
    Piped,
    Null,
}

impl Stdio {
    pub fn piped() -> Self {
        Self::Piped
    }

    pub fn null() -> Self {
        Self::Null
    }

    pub fn inherit() -> Self {
        Self::Inherit
    }
}

impl From<Stdio> for std::process::Stdio {
    fn from(s: Stdio) -> Self {
        match s {
            Stdio::Inherit => std::process::Stdio::inherit(),
            Stdio::Piped => std::process::Stdio::piped(),
            Stdio::Null => std::process::Stdio::null(),
        }
    }
}

impl From<std::process::Stdio> for Stdio {
    fn from(_s: std::process::Stdio) -> Self {
        // Can't inspect std::process::Stdio's internal state.
        // Default to Piped since the upstream code primarily uses piped() for
        // command execution (Stdio::null() for stdin, Stdio::piped() for stdout/stderr).
        // This ensures child.stdout.take() returns Some, which the exec pipeline requires.
        Stdio::Piped
    }
}
