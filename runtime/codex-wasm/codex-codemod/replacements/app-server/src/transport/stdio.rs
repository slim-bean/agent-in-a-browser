#![allow(unused_variables, unused_imports, dead_code)]

use super::TransportEvent;
use std::io::Result as IoResult;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// Stub: stdio transport is not available in the WASM build.
///
/// Returns an error indicating that stdio transport is unsupported.
pub(crate) async fn start_stdio_connection(
    transport_event_tx: mpsc::Sender<TransportEvent>,
    stdio_handles: &mut Vec<JoinHandle<()>>,
    _stdio_client_name_tx: oneshot::Sender<String>,
) -> IoResult<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "stdio transport is not available in this build",
    ))
}
