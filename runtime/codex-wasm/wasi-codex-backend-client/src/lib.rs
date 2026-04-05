//! wasip2-compatible shim for codex-backend-client.
//!
//! The backend API is not accessible from the browser. This shim provides
//! the `Client` type with stub methods that return errors.

use anyhow::Result;

pub use codex_protocol::protocol::RateLimitSnapshot;

/// Backend API client stub.
pub struct Client;

impl Client {
    /// Construct a client from auth credentials.
    /// Always fails in WASM — backend API not accessible from browser.
    pub fn from_auth(
        _base_url: String,
        _auth: &codex_login::CodexAuth,
    ) -> Result<Self> {
        anyhow::bail!("backend client not available in WASM")
    }

    /// Fetch rate limits.
    /// Always fails in WASM.
    pub async fn get_rate_limits_many(&self) -> Result<Vec<RateLimitSnapshot>> {
        anyhow::bail!("backend client not available in WASM")
    }
}
