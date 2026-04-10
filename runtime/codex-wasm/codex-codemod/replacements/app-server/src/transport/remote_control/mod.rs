//! Stub remote_control module for WASM — remote control is not available.
//! The real module depends on axum, gethostname, codex-login AuthManager, etc.
//! which do not compile for wasm32-wasip2.

use std::io;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::TransportEvent;
use codex_login::AuthManager;
use codex_state::StateRuntime;

#[derive(Clone)]
pub(crate) struct RemoteControlHandle;

impl RemoteControlHandle {
    pub(crate) fn set_enabled(&self, _enabled: bool) {}
}

pub(crate) async fn start_remote_control(
    _remote_control_url: String,
    _state_db: Option<Arc<StateRuntime>>,
    _auth_manager: Arc<AuthManager>,
    _transport_event_tx: mpsc::Sender<TransportEvent>,
    _shutdown_token: CancellationToken,
    _app_server_client_name_rx: Option<oneshot::Receiver<String>>,
    _initial_enabled: bool,
) -> io::Result<(JoinHandle<()>, RemoteControlHandle)> {
    let join_handle = tokio::spawn(async {});
    Ok((join_handle, RemoteControlHandle))
}

pub(crate) async fn validate_remote_control_auth(
    _auth_manager: &Arc<AuthManager>,
) -> io::Result<()> {
    Ok(())
}
