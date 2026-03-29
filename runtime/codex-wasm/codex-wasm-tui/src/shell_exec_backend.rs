//! Process backend that routes through the WIT shell-exec interface.
//!
//! The host (browser SharedWorker) implements `codex:agent/shell-exec`
//! by dispatching to its MCP shell tool infrastructure.

use crate::bindings::codex::tui::shell_exec;
use tokio::process_backend::{ExecRequest, ExecResponse, ProcessBackend};

pub struct WasiShellBackend;

impl ProcessBackend for WasiShellBackend {
    fn execute(&self, request: ExecRequest) -> Result<ExecResponse, String> {
        let env = shell_exec::ExecEnv {
            cwd: request.cwd,
            vars: request.env,
        };

        let args: Vec<String> = request.args;

        let result = shell_exec::exec(
            &request.program,
            &args,
            &env,
            request.stdin.as_deref(),
            request.timeout_ms,
        )?;

        Ok(ExecResponse {
            exit_code: result.exit_code,
            stdout: result.stdout,
            stderr: result.stderr,
        })
    }
}
