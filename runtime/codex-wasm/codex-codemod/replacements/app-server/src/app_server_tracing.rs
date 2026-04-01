#![allow(unused_variables, unused_imports, dead_code)]

use crate::message_processor::ConnectionSessionState;
use crate::outgoing_message::ConnectionId;
use crate::transport::AppServerTransport;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::JSONRPCRequest;
use tracing::Span;
use tracing::info_span;
use tracing::field;

/// Build a tracing span for a JSON-RPC request arriving over a transport.
pub(crate) fn request_span(
    request: &JSONRPCRequest,
    transport: AppServerTransport,
    connection_id: ConnectionId,
    session: &ConnectionSessionState,
) -> Span {
    let method = request.method.as_str();
    info_span!(
        "app_server.request",
        otel.kind = "server",
        otel.name = method,
        rpc.system = "jsonrpc",
        rpc.method = method,
        rpc.request_id = %request.id,
        app_server.connection_id = %connection_id,
        app_server.api_version = "v2",
        app_server.client_name = field::Empty,
        app_server.client_version = field::Empty,
        turn.id = field::Empty,
    )
}

/// Build a tracing span for a typed in-process request.
pub(crate) fn typed_request_span(
    request: &ClientRequest,
    connection_id: ConnectionId,
    session: &ConnectionSessionState,
) -> Span {
    let method = request.method();
    info_span!(
        "app_server.request",
        otel.kind = "server",
        otel.name = %method,
        rpc.system = "jsonrpc",
        rpc.method = %method,
        rpc.transport = "in-process",
        rpc.request_id = %request.id(),
        app_server.connection_id = %connection_id,
        app_server.api_version = "v2",
        app_server.client_name = field::Empty,
        app_server.client_version = field::Empty,
        turn.id = field::Empty,
    )
}
