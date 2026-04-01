#![allow(unused_variables, unused_imports, dead_code)]

use std::io;
use std::path::PathBuf;

/// CLI arguments for websocket auth configuration.
///
/// Stub: all fields are retained for API compatibility but the auth
/// logic (JWT validation, capability tokens) is stripped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppServerWebsocketAuthArgs {
    pub ws_auth: Option<WebsocketAuthCliMode>,
    pub ws_token_file: Option<PathBuf>,
    pub ws_shared_secret_file: Option<PathBuf>,
    pub ws_issuer: Option<String>,
    pub ws_audience: Option<String>,
    pub ws_max_clock_skew_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebsocketAuthCliMode {
    CapabilityToken,
    SignedBearerToken,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppServerWebsocketAuthSettings {
    pub config: Option<AppServerWebsocketAuthConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppServerWebsocketAuthConfig {
    CapabilityToken { token_file: PathBuf },
    SignedBearerToken {
        shared_secret_file: PathBuf,
        issuer: Option<String>,
        audience: Option<String>,
        max_clock_skew_seconds: u64,
    },
}

/// Internal auth policy representation (no-op in this stub).
#[derive(Clone, Debug, Default)]
pub(crate) struct WebsocketAuthPolicy {
    pub(crate) mode: Option<WebsocketAuthMode>,
}

#[derive(Clone, Debug)]
pub(crate) enum WebsocketAuthMode {}

impl AppServerWebsocketAuthArgs {
    pub fn try_into_settings(self) -> anyhow::Result<AppServerWebsocketAuthSettings> {
        Ok(AppServerWebsocketAuthSettings { config: None })
    }
}

pub(crate) fn policy_from_settings(
    settings: &AppServerWebsocketAuthSettings,
) -> io::Result<WebsocketAuthPolicy> {
    Ok(WebsocketAuthPolicy::default())
}

pub(crate) fn should_warn_about_unauthenticated_non_loopback_listener(
    _bind_address: std::net::SocketAddr,
    _policy: &WebsocketAuthPolicy,
) -> bool {
    false
}
