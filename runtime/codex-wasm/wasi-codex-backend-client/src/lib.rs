//! wasip2-compatible shim for codex-backend-client.
//!
//! The backend API is not accessible from the browser. This shim provides
//! the `Client` type with stub methods that return errors.

use anyhow::Result;

pub use codex_protocol::protocol::RateLimitSnapshot;

/// HTTP status code stub matching reqwest/http StatusCode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusCode(u16);

impl StatusCode {
    pub fn as_u16(self) -> u16 {
        self.0
    }
}

/// Error type for backend API requests.
#[derive(Debug)]
pub enum RequestError {
    UnexpectedStatus {
        method: String,
        url: String,
        status: StatusCode,
        content_type: String,
        body: String,
    },
    Other(anyhow::Error),
}

impl RequestError {
    pub fn status(&self) -> Option<StatusCode> {
        match self {
            RequestError::UnexpectedStatus { status, .. } => Some(*status),
            RequestError::Other(_) => None,
        }
    }
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestError::UnexpectedStatus {
                method, url, status, body, ..
            } => write!(f, "{method} {url} returned {}: {body}", status.as_u16()),
            RequestError::Other(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for RequestError {}

impl From<anyhow::Error> for RequestError {
    fn from(err: anyhow::Error) -> Self {
        RequestError::Other(err)
    }
}

/// Workspace role for the current user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRole {
    AccountOwner,
    AccountAdmin,
    StandardUser,
}

impl WorkspaceRole {
    pub fn from_api_str(value: &str) -> Option<Self> {
        match value {
            "account-owner" | "account_owner" => Some(Self::AccountOwner),
            "account-admin" | "account_admin" => Some(Self::AccountAdmin),
            "standard-user" | "standard_user" | "member" => Some(Self::StandardUser),
            _ => None,
        }
    }
}

/// Backend API client stub.
pub struct Client;

impl Client {
    /// Construct a client from auth credentials.
    /// Always fails in WASM -- backend API not accessible from browser.
    pub fn from_auth(
        _base_url: impl Into<String>,
        _auth: &codex_login::CodexAuth,
    ) -> Result<Self> {
        anyhow::bail!("backend client not available in WASM")
    }

    /// Fetch rate limits.
    /// Always fails in WASM.
    pub async fn get_rate_limits_many(&self) -> Result<Vec<RateLimitSnapshot>> {
        anyhow::bail!("backend client not available in WASM")
    }

    /// Send add-credits nudge email.
    /// Always fails in WASM.
    pub async fn send_add_credits_nudge_email(&self) -> std::result::Result<(), RequestError> {
        Err(RequestError::Other(anyhow::anyhow!(
            "backend client not available in WASM"
        )))
    }

    /// Get current workspace role.
    /// Always returns None in WASM.
    pub async fn get_current_workspace_role(&self) -> Result<Option<WorkspaceRole>> {
        Ok(None)
    }
}
