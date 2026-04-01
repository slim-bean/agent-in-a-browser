#![allow(unused_variables, unused_imports, dead_code)]

use super::TransportEvent;
use super::auth::WebsocketAuthPolicy;
use crate::outgoing_message::ConnectionId;
use std::io::Result as IoResult;
use std::net::SocketAddr;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Stub: WebSocket acceptor is not available in the WASM build.
///
/// Returns an error indicating that WebSocket transport is unsupported.
pub(crate) async fn start_websocket_acceptor(
    bind_address: SocketAddr,
    transport_event_tx: mpsc::Sender<TransportEvent>,
    shutdown_token: CancellationToken,
    auth_policy: WebsocketAuthPolicy,
) -> IoResult<JoinHandle<()>> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "WebSocket transport is not available in this build",
    ))
}
