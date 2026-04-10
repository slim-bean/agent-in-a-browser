//! wasip2-compatible MCP client shim.
//!
//! Implements the MCP streamable HTTP transport (JSON-RPC over HTTP POST)
//! using the wasi-reqwest shim. Process spawning, OAuth, and browser
//! interactions remain stubbed out for WASM.

#![allow(dead_code, unused_variables, unused_imports)]

use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};

// Re-export types from rmcp model (codex-core uses these via us AND directly)
pub use rmcp::model::ElicitationAction;
pub use rmcp::model::CreateElicitationRequestParams;

// Re-export from codex_protocol
pub use codex_protocol::protocol::McpAuthStatus;

/// Type alias matching upstream.
pub type Elicitation = CreateElicitationRequestParams;

/// Callback for handling elicitation requests.
pub type SendElicitation = Box<
    dyn Fn(rmcp::model::RequestId, Elicitation) -> BoxFuture<'static, Result<ElicitationResponse>>
        + Send
        + Sync,
>;

/// Response to an elicitation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationResponse {
    pub action: ElicitationAction,
    pub content: Option<serde_json::Value>,
    #[serde(rename = "_meta")]
    pub meta: Option<serde_json::Value>,
}

/// Tool with connector metadata.
pub struct ToolWithConnectorId {
    pub tool: rmcp::model::Tool,
    pub connector_id: Option<String>,
    pub connector_name: Option<String>,
    pub connector_description: Option<String>,
}

/// Result of listing tools with connector IDs.
pub struct ListToolsWithConnectorIdResult {
    pub next_cursor: Option<String>,
    pub tools: Vec<ToolWithConnectorId>,
}

/// OAuth credential storage mode — re-exported from codex_config to ensure type identity.
pub use codex_config::types::OAuthCredentialsStoreMode;

/// Stored OAuth tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredOAuthTokens {
    pub server_name: String,
    pub url: String,
    pub client_id: String,
    pub token_response: serde_json::Value,
    pub expires_at: Option<u64>,
}

/// Wrapped OAuth token response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrappedOAuthTokenResponse(pub serde_json::Value);

/// OAuth provider error.
#[derive(Debug)]
pub struct OAuthProviderError {
    pub error: Option<String>,
    pub error_description: Option<String>,
}

impl OAuthProviderError {
    pub fn new(error: Option<String>, error_description: Option<String>) -> Self {
        Self { error, error_description }
    }
}

impl std::fmt::Display for OAuthProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OAuth error: {:?}", self.error)
    }
}

impl std::error::Error for OAuthProviderError {}

/// Handle for an in-progress OAuth login.
pub struct OauthLoginHandle {
    authorization_url: String,
}

impl OauthLoginHandle {
    pub fn authorization_url(&self) -> &str {
        &self.authorization_url
    }

    pub fn into_parts(self) -> (String, tokio::sync::oneshot::Receiver<Result<()>>) {
        let (_tx, rx) = tokio::sync::oneshot::channel();
        (self.authorization_url, rx)
    }

    pub async fn wait(self) -> Result<()> {
        anyhow::bail!("OAuth login not available in WASM")
    }
}

/// OAuth discovery result.
#[derive(Debug, Clone)]
pub struct StreamableHttpOAuthDiscovery {
    pub scopes_supported: Option<Vec<String>>,
}

/// Save OAuth tokens — no-op in WASM.
pub fn save_oauth_tokens(
    _server_name: &str,
    _tokens: &StoredOAuthTokens,
    _store_mode: OAuthCredentialsStoreMode,
) -> Result<()> {
    Ok(())
}

/// Delete OAuth tokens — no-op in WASM.
pub fn delete_oauth_tokens(
    _server_name: &str,
    _url: &str,
    _store_mode: OAuthCredentialsStoreMode,
) -> Result<bool> {
    Ok(false)
}

/// Perform OAuth login silently — not available in WASM.
#[allow(clippy::too_many_arguments)]
pub async fn perform_oauth_login_silent(
    _server_name: &str,
    _server_url: &str,
    _store_mode: OAuthCredentialsStoreMode,
    _http_headers: Option<HashMap<String, String>>,
    _env_http_headers: Option<HashMap<String, String>>,
    _scopes: &[String],
    _oauth_resource: Option<&str>,
    _callback_port: Option<u16>,
    _callback_url: Option<&str>,
) -> Result<()> {
    anyhow::bail!("OAuth login not available in WASM")
}

/// Perform OAuth login — not available in WASM.
pub async fn perform_oauth_login(
    _server_name: &str,
    _server_url: &str,
    _store_mode: OAuthCredentialsStoreMode,
    _http_headers: Option<HashMap<String, String>>,
    _env_http_headers: Option<HashMap<String, String>>,
    _scopes: &[String],
    _oauth_resource: Option<&str>,
    _callback_port: Option<u16>,
    _callback_url: Option<&str>,
) -> Result<()> {
    anyhow::bail!("OAuth login not available in WASM")
}

/// Perform OAuth login returning URL — not available in WASM.
pub async fn perform_oauth_login_return_url(
    _server_name: &str,
    _server_url: &str,
    _store_mode: OAuthCredentialsStoreMode,
    _http_headers: Option<HashMap<String, String>>,
    _env_http_headers: Option<HashMap<String, String>>,
    _scopes: &[String],
    _oauth_resource: Option<&str>,
    _timeout_secs: Option<i64>,
    _callback_port: Option<u16>,
    _callback_url: Option<&str>,
) -> Result<OauthLoginHandle> {
    anyhow::bail!("OAuth login not available in WASM")
}

/// Determine auth status — returns Unsupported in WASM.
pub async fn determine_streamable_http_auth_status(
    _server_name: &str,
    _url: &str,
    _bearer_token_env_var: Option<&str>,
    _http_headers: Option<HashMap<String, String>>,
    _env_http_headers: Option<HashMap<String, String>>,
    _store_mode: OAuthCredentialsStoreMode,
) -> Result<McpAuthStatus> {
    Ok(McpAuthStatus::Unsupported)
}

/// Check if OAuth is supported — always false in WASM.
pub async fn supports_oauth_login(_url: &str) -> Result<bool> {
    Ok(false)
}

/// Discover OAuth configuration — returns None in WASM.
pub async fn discover_streamable_http_oauth(
    _url: &str,
    _http_headers: Option<HashMap<String, String>>,
    _env_http_headers: Option<HashMap<String, String>>,
) -> Result<Option<StreamableHttpOAuthDiscovery>> {
    Ok(None)
}

/// Extract the `result` field from a JSON-RPC response, or return an error.
fn extract_result(response: serde_json::Value) -> Result<serde_json::Value> {
    if let Some(error) = response.get("error") {
        let msg = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        anyhow::bail!("JSON-RPC error {code}: {msg}");
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("missing result in JSON-RPC response"))
}

/// MCP client backed by streamable HTTP (JSON-RPC over HTTP POST).
pub struct RmcpClient {
    url: String,
    bearer_token: Option<String>,
    http_headers: HashMap<String, String>,
    session_id: std::sync::Mutex<Option<String>>,
    next_id: std::sync::atomic::AtomicU64,
}

impl RmcpClient {
    /// Create a stdio MCP client — not available in WASM.
    pub async fn new_stdio_client(
        _program: OsString,
        _args: Vec<OsString>,
        _env: Option<HashMap<String, String>>,
        _env_vars: &[String],
        _cwd: Option<PathBuf>,
    ) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "stdio MCP client not available in WASM",
        ))
    }

    /// Create a streamable HTTP MCP client.
    pub async fn new_streamable_http_client(
        _server_name: &str,
        url: &str,
        bearer_token: Option<String>,
        http_headers: Option<HashMap<String, String>>,
        env_http_headers: Option<HashMap<String, String>>,
        _store_mode: OAuthCredentialsStoreMode,
    ) -> Result<Self> {
        let mut headers = http_headers.unwrap_or_default();
        if let Some(env) = env_http_headers {
            headers.extend(env);
        }
        Ok(Self {
            url: url.to_string(),
            bearer_token,
            http_headers: headers,
            session_id: std::sync::Mutex::new(None),
            next_id: std::sync::atomic::AtomicU64::new(1),
        })
    }

    /// Build a POST request with all common headers applied.
    fn build_request(&self) -> reqwest::RequestBuilder {
        let mut builder = reqwest::Client::new()
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream");

        if let Some(token) = &self.bearer_token {
            builder = builder.header("Authorization", format!("Bearer {token}"));
        }
        for (k, v) in &self.http_headers {
            builder = builder.header(k.as_str(), v.as_str());
        }
        if let Ok(guard) = self.session_id.lock() {
            if let Some(sid) = guard.as_ref() {
                builder = builder.header("Mcp-Session-Id", sid.as_str());
            }
        }
        builder
    }

    /// Save the session ID from a response if the server provides one.
    fn save_session_id(&self, headers: &reqwest::header::HeaderMap) {
        if let Some(sid) = headers.get("mcp-session-id") {
            if let Ok(sid_str) = sid.to_str() {
                if let Ok(mut guard) = self.session_id.lock() {
                    *guard = Some(sid_str.to_string());
                }
            }
        }
    }

    /// Send a JSON-RPC request (with `id`) and parse the response.
    async fn send_jsonrpc(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params.unwrap_or(serde_json::Value::Null),
        });

        let response = self
            .build_request()
            .body(serde_json::to_string(&request)?)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("HTTP request failed: {e}"))?;

        self.save_session_id(response.headers());

        let body = response
            .text()
            .await
            .map_err(|e| anyhow::anyhow!("failed to read response body: {e}"))?;

        // Handle SSE response (text/event-stream) — extract last data event.
        if body.starts_with("data:") || body.contains("\ndata:") {
            let last_data = body
                .lines()
                .filter(|l| l.starts_with("data:"))
                .last()
                .map(|l| l.strip_prefix("data:").unwrap_or(l).trim())
                .unwrap_or(&body);
            let parsed: serde_json::Value = serde_json::from_str(last_data)?;
            return extract_result(parsed);
        }

        let parsed: serde_json::Value = serde_json::from_str(&body)?;
        extract_result(parsed)
    }

    /// Send a JSON-RPC notification (no `id`, no response expected).
    async fn send_notification(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<()> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params.unwrap_or(serde_json::Value::Null),
        });

        let _ = self
            .build_request()
            .body(serde_json::to_string(&request)?)
            .send()
            .await;

        Ok(())
    }

    /// Initialize the MCP connection.
    pub async fn initialize(
        &self,
        params: rmcp::model::InitializeRequestParams,
        _timeout: Option<Duration>,
        _send_elicitation: SendElicitation,
    ) -> Result<rmcp::model::InitializeResult> {
        let result = self
            .send_jsonrpc("initialize", Some(serde_json::to_value(&params)?))
            .await?;
        let init_result: rmcp::model::InitializeResult = serde_json::from_value(result)?;

        // Send initialized notification per MCP protocol.
        self.send_notification("notifications/initialized", None)
            .await
            .ok();

        Ok(init_result)
    }

    /// List available tools.
    pub async fn list_tools(
        &self,
        params: Option<rmcp::model::PaginatedRequestParams>,
        _timeout: Option<Duration>,
    ) -> Result<rmcp::model::ListToolsResult> {
        let result = self
            .send_jsonrpc(
                "tools/list",
                params.map(|p| serde_json::to_value(&p).unwrap()),
            )
            .await?;
        Ok(serde_json::from_value(result)?)
    }

    /// List tools with connector IDs.
    pub async fn list_tools_with_connector_ids(
        &self,
        params: Option<rmcp::model::PaginatedRequestParams>,
        timeout: Option<Duration>,
    ) -> Result<ListToolsWithConnectorIdResult> {
        let result = self.list_tools(params, timeout).await?;
        let tools = result
            .tools
            .into_iter()
            .map(|tool| ToolWithConnectorId {
                tool,
                connector_id: None,
                connector_name: None,
                connector_description: None,
            })
            .collect();
        Ok(ListToolsWithConnectorIdResult {
            next_cursor: result.next_cursor,
            tools,
        })
    }

    /// List available resources.
    pub async fn list_resources(
        &self,
        params: Option<rmcp::model::PaginatedRequestParams>,
        _timeout: Option<Duration>,
    ) -> Result<rmcp::model::ListResourcesResult> {
        let result = self
            .send_jsonrpc(
                "resources/list",
                params.map(|p| serde_json::to_value(&p).unwrap()),
            )
            .await?;
        Ok(serde_json::from_value(result)?)
    }

    /// List resource templates.
    pub async fn list_resource_templates(
        &self,
        params: Option<rmcp::model::PaginatedRequestParams>,
        _timeout: Option<Duration>,
    ) -> Result<rmcp::model::ListResourceTemplatesResult> {
        let result = self
            .send_jsonrpc(
                "resources/templates/list",
                params.map(|p| serde_json::to_value(&p).unwrap()),
            )
            .await?;
        Ok(serde_json::from_value(result)?)
    }

    /// Read a resource.
    pub async fn read_resource(
        &self,
        params: rmcp::model::ReadResourceRequestParams,
        _timeout: Option<Duration>,
    ) -> Result<rmcp::model::ReadResourceResult> {
        let result = self
            .send_jsonrpc("resources/read", Some(serde_json::to_value(&params)?))
            .await?;
        Ok(serde_json::from_value(result)?)
    }

    /// Call a tool.
    pub async fn call_tool(
        &self,
        name: String,
        arguments: Option<serde_json::Value>,
        _meta: Option<serde_json::Value>,
        _timeout: Option<Duration>,
    ) -> Result<rmcp::model::CallToolResult> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments.unwrap_or(serde_json::Value::Object(Default::default())),
        });
        let result = self.send_jsonrpc("tools/call", Some(params)).await?;
        Ok(serde_json::from_value(result)?)
    }

    /// Send a custom notification.
    pub async fn send_custom_notification(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<()> {
        self.send_notification(method, params).await
    }

    /// Send a custom request.
    pub async fn send_custom_request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        self.send_jsonrpc(method, params).await
    }
}
