//! Stub — shell-escalation for wasip2.
#![allow(dead_code, unused_variables, unused_imports)]
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use anyhow::Result;
pub use codex_protocol::approvals::EscalationPermissions;
pub use codex_protocol::approvals::Permissions;

pub const ESCALATE_SOCKET_ENV_VAR: &str = "CODEX_ESCALATE_SOCKET";
pub const EXEC_WRAPPER_ENV_VAR: &str = "EXEC_WRAPPER";
pub const LEGACY_BASH_EXEC_WRAPPER_ENV_VAR: &str = "BASH_EXEC_WRAPPER";

#[derive(Debug, Clone)]
pub enum EscalationDecision { Run, Escalate(EscalationExecution), Deny { reason: Option<String> } }
#[derive(Debug, Clone)]
pub enum EscalationExecution { Unsandboxed, TurnDefault, Permissions(EscalationPermissions) }
#[derive(Debug, Clone)]
pub enum EscalateAction { Run, Escalate, Deny { reason: Option<String> } }

impl EscalationDecision {
    pub fn run() -> Self { Self::Run }
    pub fn escalate(execution: EscalationExecution) -> Self { Self::Escalate(execution) }
    pub fn deny(reason: Option<String>) -> Self { Self::Deny { reason } }
}

pub struct ExecParams { pub command: String, pub workdir: String, pub timeout_ms: Option<u64>, pub login: Option<bool> }
pub struct ExecResult { pub exit_code: i32, pub stdout: String, pub stderr: String, pub output: String, pub duration: Duration, pub timed_out: bool }
pub struct PreparedExec { pub command: Vec<String>, pub cwd: PathBuf, pub env: HashMap<String, String>, pub arg0: Option<String> }

#[async_trait::async_trait]
pub trait EscalationPolicy: Send + Sync {
    async fn determine_action(&self, file: &codex_utils_absolute_path::AbsolutePathBuf, argv: &[String], workdir: &codex_utils_absolute_path::AbsolutePathBuf) -> Result<EscalationDecision>;
}

#[async_trait::async_trait]
pub trait ShellCommandExecutor: Send + Sync {
    async fn run(&self, command: Vec<String>, cwd: PathBuf, env_overlay: HashMap<String, String>, cancel_rx: tokio_util::sync::CancellationToken, after_spawn: Option<Box<dyn FnOnce() + Send>>) -> Result<ExecResult>;
    async fn prepare_escalated_exec(&self, program: &codex_utils_absolute_path::AbsolutePathBuf, argv: &[String], workdir: &codex_utils_absolute_path::AbsolutePathBuf, env: HashMap<String, String>, execution: EscalationExecution) -> Result<PreparedExec>;
}

pub struct EscalateServer;
impl EscalateServer {
    pub fn new<Policy: EscalationPolicy + Send + Sync + 'static>(_bash_path: PathBuf, _execve_wrapper: PathBuf, _policy: Policy) -> Self { Self }
    pub async fn exec(&self, _params: ExecParams, _cancel_rx: &tokio_util::sync::CancellationToken, _executor: std::sync::Arc<dyn ShellCommandExecutor>) -> Result<ExecResult> { anyhow::bail!("not available in WASM") }
    pub fn start_session(&self, _parent: tokio_util::sync::CancellationToken, _executor: std::sync::Arc<dyn ShellCommandExecutor>) -> Result<EscalationSession> { anyhow::bail!("not available in WASM") }
}

pub struct EscalationSession { _private: () }
impl EscalationSession {
    pub fn env(&self) -> &HashMap<String, String> { static E: std::sync::LazyLock<HashMap<String, String>> = std::sync::LazyLock::new(HashMap::new); &E }
    pub fn close_client_socket(&self) {}
}
impl std::fmt::Debug for EscalationSession { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_struct("EscalationSession").finish() } }

#[derive(Clone)]
pub struct Stopwatch { _ct: tokio_util::sync::CancellationToken }
impl Stopwatch {
    pub fn new(_limit: Duration) -> Self { Self { _ct: tokio_util::sync::CancellationToken::new() } }
    pub fn unlimited() -> Self { Self { _ct: tokio_util::sync::CancellationToken::new() } }
    pub fn elapsed(&self) -> Duration { Duration::ZERO }
    pub fn cancellation_token(&self) -> &tokio_util::sync::CancellationToken { &self._ct }
    pub async fn pause_for<F: std::future::Future>(&self, fut: F) -> F::Output { fut.await }
}
impl Default for Stopwatch { fn default() -> Self { Self { _ct: tokio_util::sync::CancellationToken::new() } } }

pub fn run_shell_escalation_execve_wrapper() {}
pub fn main_execve_wrapper() {}
