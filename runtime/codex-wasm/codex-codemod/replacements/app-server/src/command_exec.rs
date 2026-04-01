#![allow(unused_variables, unused_imports, dead_code)]

use codex_app_server_protocol::CommandExecResizeParams;
use codex_app_server_protocol::CommandExecResizeResponse;
use codex_app_server_protocol::CommandExecTerminalSize;
use codex_app_server_protocol::CommandExecTerminateParams;
use codex_app_server_protocol::CommandExecTerminateResponse;
use codex_app_server_protocol::CommandExecWriteParams;
use codex_app_server_protocol::CommandExecWriteResponse;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_core::sandboxing::ExecRequest;
use codex_core::config::StartedNetworkProxy;
use codex_utils_pty::TerminalSize;
use std::sync::Arc;

use crate::error_code::INVALID_REQUEST_ERROR_CODE;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::ConnectionRequestId;
use crate::outgoing_message::OutgoingMessageSender;

pub(crate) struct StartCommandExecParams {
    pub(crate) outgoing: Arc<OutgoingMessageSender>,
    pub(crate) request_id: ConnectionRequestId,
    pub(crate) process_id: Option<String>,
    pub(crate) exec_request: ExecRequest,
    pub(crate) started_network_proxy: Option<StartedNetworkProxy>,
    pub(crate) tty: bool,
    pub(crate) stream_stdin: bool,
    pub(crate) stream_stdout_stderr: bool,
    pub(crate) output_bytes_cap: Option<usize>,
    pub(crate) size: Option<TerminalSize>,
}

/// No-op command execution manager for the WASM build.
///
/// All operations return an error indicating that command execution is
/// not supported.
#[derive(Clone, Default)]
pub(crate) struct CommandExecManager;

impl CommandExecManager {
    pub(crate) async fn start(
        &self,
        params: StartCommandExecParams,
    ) -> Result<(), JSONRPCErrorError> {
        Err(JSONRPCErrorError {
            code: INVALID_REQUEST_ERROR_CODE,
            message: "command/exec is not available in this build".to_string(),
            data: None,
        })
    }

    pub(crate) async fn write(
        &self,
        request_id: ConnectionRequestId,
        params: CommandExecWriteParams,
    ) -> Result<CommandExecWriteResponse, JSONRPCErrorError> {
        Err(JSONRPCErrorError {
            code: INVALID_REQUEST_ERROR_CODE,
            message: "command/exec is not available in this build".to_string(),
            data: None,
        })
    }

    pub(crate) async fn terminate(
        &self,
        request_id: ConnectionRequestId,
        params: CommandExecTerminateParams,
    ) -> Result<CommandExecTerminateResponse, JSONRPCErrorError> {
        Err(JSONRPCErrorError {
            code: INVALID_REQUEST_ERROR_CODE,
            message: "command/exec is not available in this build".to_string(),
            data: None,
        })
    }

    pub(crate) async fn resize(
        &self,
        request_id: ConnectionRequestId,
        params: CommandExecResizeParams,
    ) -> Result<CommandExecResizeResponse, JSONRPCErrorError> {
        Err(JSONRPCErrorError {
            code: INVALID_REQUEST_ERROR_CODE,
            message: "command/exec is not available in this build".to_string(),
            data: None,
        })
    }

    pub(crate) async fn connection_closed(&self, connection_id: ConnectionId) {}
}

pub(crate) fn terminal_size_from_protocol(
    size: CommandExecTerminalSize,
) -> Result<TerminalSize, JSONRPCErrorError> {
    if size.rows == 0 || size.cols == 0 {
        return Err(JSONRPCErrorError {
            code: INVALID_REQUEST_ERROR_CODE,
            message: "command/exec size rows and cols must be greater than 0".to_string(),
            data: None,
        });
    }
    Ok(TerminalSize {
        rows: size.rows,
        cols: size.cols,
    })
}
