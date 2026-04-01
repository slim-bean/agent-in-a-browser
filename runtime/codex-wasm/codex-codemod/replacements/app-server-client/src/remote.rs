#![allow(unused_variables, unused_imports, dead_code)]

use std::collections::VecDeque;
use std::io::Error as IoError;
use std::io::ErrorKind;
use std::io::Result as IoResult;

use crate::AppServerEvent;
use crate::RequestResult;
use crate::TypedRequestError;
use crate::request_method_name;
use crate::server_notification_requires_delivery;
use codex_app_server_protocol::ClientNotification;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::Result as JsonRpcResult;
use serde::de::DeserializeOwned;
use url::Url;

/// Arguments for connecting to a remote app-server over WebSocket.
#[derive(Debug, Clone)]
pub struct RemoteAppServerConnectArgs {
    pub websocket_url: String,
    pub auth_token: Option<String>,
    pub client_name: String,
    pub client_version: String,
    pub experimental_api: bool,
    pub opt_out_notification_methods: Vec<String>,
    pub channel_capacity: usize,
}

/// Stub remote app-server client.
///
/// In the WASM build, remote WebSocket connections are not supported.
/// All methods return connection-unavailable errors.
pub struct RemoteAppServerClient {
    _private: (),
}

/// Stub request handle for the remote client.
#[derive(Clone)]
pub struct RemoteAppServerRequestHandle {
    _private: (),
}

pub(crate) fn websocket_url_supports_auth_token(url: &Url) -> bool {
    match (url.scheme(), url.host()) {
        ("wss", Some(_)) => true,
        ("ws", Some(url::Host::Domain(domain))) => domain.eq_ignore_ascii_case("localhost"),
        ("ws", Some(url::Host::Ipv4(addr))) => addr.is_loopback(),
        ("ws", Some(url::Host::Ipv6(addr))) => addr.is_loopback(),
        _ => false,
    }
}

impl RemoteAppServerClient {
    pub async fn connect(args: RemoteAppServerConnectArgs) -> IoResult<Self> {
        Err(IoError::new(
            ErrorKind::Unsupported,
            "remote WebSocket connections are not available in this build",
        ))
    }

    pub fn request_handle(&self) -> RemoteAppServerRequestHandle {
        RemoteAppServerRequestHandle { _private: () }
    }

    pub async fn request(&self, request: ClientRequest) -> IoResult<RequestResult> {
        Err(IoError::new(
            ErrorKind::NotConnected,
            "remote app-server client is not connected",
        ))
    }

    pub async fn request_typed<T>(&self, request: ClientRequest) -> Result<T, TypedRequestError>
    where
        T: DeserializeOwned,
    {
        let method = request_method_name(&request);
        Err(TypedRequestError::Transport {
            method,
            source: IoError::new(
                ErrorKind::NotConnected,
                "remote app-server client is not connected",
            ),
        })
    }

    pub async fn notify(&self, notification: ClientNotification) -> IoResult<()> {
        Err(IoError::new(
            ErrorKind::NotConnected,
            "remote app-server client is not connected",
        ))
    }

    pub async fn resolve_server_request(
        &self,
        request_id: RequestId,
        result: JsonRpcResult,
    ) -> IoResult<()> {
        Err(IoError::new(
            ErrorKind::NotConnected,
            "remote app-server client is not connected",
        ))
    }

    pub async fn reject_server_request(
        &self,
        request_id: RequestId,
        error: JSONRPCErrorError,
    ) -> IoResult<()> {
        Err(IoError::new(
            ErrorKind::NotConnected,
            "remote app-server client is not connected",
        ))
    }

    pub async fn next_event(&mut self) -> Option<AppServerEvent> {
        None
    }

    pub async fn shutdown(self) -> IoResult<()> {
        Ok(())
    }
}

impl RemoteAppServerRequestHandle {
    pub async fn request(&self, request: ClientRequest) -> IoResult<RequestResult> {
        Err(IoError::new(
            ErrorKind::NotConnected,
            "remote app-server client is not connected",
        ))
    }

    pub async fn request_typed<T>(&self, request: ClientRequest) -> Result<T, TypedRequestError>
    where
        T: DeserializeOwned,
    {
        let method = request_method_name(&request);
        Err(TypedRequestError::Transport {
            method,
            source: IoError::new(
                ErrorKind::NotConnected,
                "remote app-server client is not connected",
            ),
        })
    }
}
