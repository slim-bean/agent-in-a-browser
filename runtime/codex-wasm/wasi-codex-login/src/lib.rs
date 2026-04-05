#![allow(dead_code, unused_variables, unused_imports)]
//! Stub for codex-login in wasip2 — auth types with no-op implementations.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Auth env-var constants
// ---------------------------------------------------------------------------

pub const OPENAI_API_KEY_ENV_VAR: &str = "OPENAI_API_KEY";
pub const CODEX_API_KEY_ENV_VAR: &str = "CODEX_API_KEY";
pub const REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR: &str = "CODEX_REFRESH_TOKEN_URL_OVERRIDE";
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const DEFAULT_ISSUER: &str = "https://auth.openai.com";

// ---------------------------------------------------------------------------
// AuthMode (mirrors codex_app_server_protocol::AuthMode)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    ApiKey,
    Chatgpt,
    #[serde(rename = "chatgptAuthTokens")]
    ChatgptAuthTokens,
}

impl std::fmt::Display for AuthMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApiKey => write!(f, "apikey"),
            Self::Chatgpt => write!(f, "chatgpt"),
            Self::ChatgptAuthTokens => write!(f, "chatgptAuthTokens"),
        }
    }
}

// ---------------------------------------------------------------------------
// AuthManager
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct AuthManager {
    auth: std::sync::Arc<std::sync::Mutex<Option<CodexAuth>>>,
    codex_home: std::sync::Arc<std::sync::Mutex<Option<PathBuf>>>,
}

impl AuthManager {
    pub fn new() -> Self {
        Self {
            auth: std::sync::Arc::new(std::sync::Mutex::new(None)),
            codex_home: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn shared(
        codex_home: std::path::PathBuf,
        _enable_codex_api_key_env: bool,
        _auth_credentials_store_mode: AuthCredentialsStoreMode,
    ) -> Arc<Self> {
        let mgr = Self::new();
        *mgr.codex_home.lock().unwrap_or_else(|e| e.into_inner()) = Some(codex_home.clone());
        // Try to load from auth.json on disk (OPFS) with full token data
        if let Ok(Some(auth)) =
            CodexAuth::from_auth_storage(&codex_home, AuthCredentialsStoreMode::File)
        {
            mgr.set_auth(auth);
        }
        // Also check env var
        if mgr.auth_cached().is_none() {
            if let Some(key) = read_openai_api_key_from_env() {
                mgr.set_auth(CodexAuth::from_api_key(key));
            }
        }
        Arc::new(mgr)
    }

    pub fn from_auth_for_testing(auth: CodexAuth) -> Arc<Self> {
        let mgr = Self::new();
        mgr.set_auth(auth);
        Arc::new(mgr)
    }

    pub fn from_auth_for_testing_with_home(
        auth: CodexAuth,
        _codex_home: std::path::PathBuf,
    ) -> Arc<Self> {
        let mgr = Self::new();
        mgr.set_auth(auth);
        Arc::new(mgr)
    }

    pub fn reload(&self) -> bool {
        let home = self
            .codex_home
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(codex_home) = home {
            if let Ok(Some(auth_json)) = auth::load_auth_dot_json(&codex_home) {
                if let Some(key) = auth_json.openai_api_key {
                    self.set_auth(CodexAuth::from_api_key(key));
                    return true;
                }
            }
        }
        self.auth_cached().is_some()
    }

    pub fn codex_api_key_env_enabled(&self) -> bool {
        false
    }

    pub async fn auth(&self) -> Option<CodexAuth> {
        self.auth_cached()
    }

    pub fn auth_cached(&self) -> Option<CodexAuth> {
        self.auth.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_auth(&self, auth: CodexAuth) {
        *self.auth.lock().unwrap_or_else(|e| e.into_inner()) = Some(auth);
    }

    pub fn auth_mode(&self) -> Option<AuthMode> {
        self.auth_cached().map(|a| a.auth_mode())
    }

    pub fn unauthorized_recovery(self: &Arc<Self>) -> auth::UnauthorizedRecovery {
        auth::UnauthorizedRecovery
    }
}

// ---------------------------------------------------------------------------
// CodexAuth
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CodexAuth {
    pub api_key: Option<String>,
    pub chatgpt_session_token: Option<String>,
    #[serde(skip)]
    token_data: Option<TokenData>,
}

impl CodexAuth {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            api_key: Some(api_key.into()),
            chatgpt_session_token: None,
            token_data: None,
        }
    }

    /// Create with full token data from auth.json.
    fn with_token_data(api_key: String, token_data: Option<TokenData>) -> Self {
        Self {
            api_key: Some(api_key),
            chatgpt_session_token: None,
            token_data,
        }
    }

    pub fn create_dummy_chatgpt_auth_for_testing() -> Self {
        Self {
            api_key: None,
            chatgpt_session_token: Some("dummy".to_string()),
            token_data: None,
        }
    }

    pub fn is_chatgpt_auth(&self) -> bool {
        self.chatgpt_session_token.is_some()
    }

    pub fn is_api_key_auth(&self) -> bool {
        self.api_key.is_some()
    }

    pub fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }

    pub fn bearer_token(&self) -> Option<&str> {
        self.api_key.as_deref()
    }

    pub fn auth_mode(&self) -> AuthMode {
        if self.api_key.is_some() {
            AuthMode::ApiKey
        } else {
            AuthMode::Chatgpt
        }
    }

    pub fn get_account_id(&self) -> Option<String> {
        self.token_data_cached()
            .and_then(|td| td.id_token.chatgpt_account_id.clone())
    }

    pub fn get_account_email(&self) -> Option<String> {
        self.token_data_cached()
            .and_then(|td| td.id_token.email.clone())
    }

    pub fn is_external_chatgpt_tokens(&self) -> bool {
        false
    }

    pub fn get_token(&self) -> Result<String, std::io::Error> {
        self.api_key
            .clone()
            .ok_or_else(|| std::io::Error::other("no token available"))
    }

    pub fn get_token_data(&self) -> Result<TokenData, std::io::Error> {
        self.token_data_cached()
            .ok_or_else(|| std::io::Error::other("Token data is not available."))
    }

    fn token_data_cached(&self) -> Option<TokenData> {
        self.token_data.clone()
    }

    /// Construct from saved auth storage — reads auth.json from OPFS.
    pub fn from_auth_storage(
        codex_home: &std::path::Path,
        _store_mode: AuthCredentialsStoreMode,
    ) -> std::io::Result<Option<Self>> {
        match auth::load_auth_dot_json(codex_home)? {
            Some(auth_json) => {
                if let Some(key) = auth_json.openai_api_key {
                    // Try to decode token data from the saved tokens
                    let td = auth_json
                        .tokens
                        .as_ref()
                        .and_then(|t| token_data::decode_token_data_from_json(t));
                    Ok(Some(Self::with_token_data(key, td)))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    pub fn account_plan_type(&self) -> Option<token_data::PlanType> {
        self.token_data_cached()
            .and_then(|td| td.id_token.chatgpt_plan_type)
    }

    pub fn get_chatgpt_user_id(&self) -> Option<String> {
        self.token_data_cached()
            .and_then(|td| td.id_token.chatgpt_user_id)
    }
}

// ---------------------------------------------------------------------------
// Top-level helper functions (re-exported from auth:: in upstream)
// ---------------------------------------------------------------------------

pub fn read_openai_api_key_from_env() -> Option<String> {
    std::env::var(OPENAI_API_KEY_ENV_VAR)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn read_codex_api_key_from_env() -> Option<String> {
    std::env::var(CODEX_API_KEY_ENV_VAR)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

// ---------------------------------------------------------------------------
// auth module
// ---------------------------------------------------------------------------

pub mod auth {
    pub use super::AuthConfig;
    pub use super::AuthCredentialsStoreMode;
    pub use super::AuthMode;
    pub use super::CodexAuth;
    pub use super::ForcedLoginMethod;
    pub use super::read_openai_api_key_from_env;

    use thiserror::Error;

    // -- RefreshTokenFailedError / RefreshTokenFailedReason --

    #[derive(Debug, Clone, PartialEq, Eq, Error)]
    #[error("{message}")]
    pub struct RefreshTokenFailedError {
        pub reason: RefreshTokenFailedReason,
        pub message: String,
    }

    impl RefreshTokenFailedError {
        pub fn new(reason: RefreshTokenFailedReason, message: impl Into<String>) -> Self {
            Self {
                reason,
                message: message.into(),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum RefreshTokenFailedReason {
        Expired,
        Exhausted,
        Revoked,
        Other,
    }

    impl std::fmt::Display for RefreshTokenFailedReason {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Expired => write!(f, "expired"),
                Self::Exhausted => write!(f, "exhausted"),
                Self::Revoked => write!(f, "revoked"),
                Self::Other => write!(f, "other"),
            }
        }
    }

    // -- RefreshTokenError --

    #[derive(Debug, Error)]
    pub enum RefreshTokenError {
        #[error("{0}")]
        Permanent(#[from] RefreshTokenFailedError),
        #[error(transparent)]
        Transient(#[from] std::io::Error),
    }

    impl RefreshTokenError {
        pub fn failed_reason(&self) -> Option<RefreshTokenFailedReason> {
            match self {
                Self::Permanent(error) => Some(error.reason),
                Self::Transient(_) => None,
            }
        }
    }

    impl From<RefreshTokenError> for std::io::Error {
        fn from(err: RefreshTokenError) -> Self {
            match err {
                RefreshTokenError::Permanent(failed) => std::io::Error::other(failed),
                RefreshTokenError::Transient(inner) => inner,
            }
        }
    }

    // -- UnauthorizedRecovery (stub) --

    #[derive(Debug)]
    pub struct UnauthorizedRecovery;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct UnauthorizedRecoveryStepResult {
        auth_state_changed: Option<bool>,
    }

    impl UnauthorizedRecoveryStepResult {
        pub fn auth_state_changed(&self) -> Option<bool> {
            self.auth_state_changed
        }
    }

    impl UnauthorizedRecovery {
        pub fn is_applicable(&self) -> bool {
            false
        }

        pub fn applicability_reason(&self) -> &'static str {
            "unauthorized recovery is not available in WASM"
        }

        pub fn has_next(&self) -> bool {
            false
        }

        pub fn unavailable_reason(&self) -> &'static str {
            "unauthorized recovery is not available in WASM"
        }

        pub fn mode_name(&self) -> &'static str {
            "not available in WASM"
        }

        pub fn step_name(&self) -> &'static str {
            "not available in WASM"
        }

        pub async fn next(&mut self) -> Result<UnauthorizedRecoveryStepResult, RefreshTokenError> {
            Ok(UnauthorizedRecoveryStepResult {
                auth_state_changed: None,
            })
        }
    }

    // -- ExternalAuth types --

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ExternalAuthTokens {
        pub access_token: String,
        pub chatgpt_account_id: String,
        pub chatgpt_plan_type: Option<String>,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum ExternalAuthRefreshReason {
        Unauthorized,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ExternalAuthRefreshContext {
        pub reason: ExternalAuthRefreshReason,
        pub previous_account_id: Option<String>,
    }

    #[async_trait::async_trait]
    pub trait ExternalAuthRefresher: Send + Sync {
        async fn refresh(
            &self,
            context: ExternalAuthRefreshContext,
        ) -> std::io::Result<ExternalAuthTokens>;
    }

    // -- Misc stubs expected by upstream re-exports --

    pub fn login_with_chatgpt_auth_tokens(
        _codex_home: &std::path::Path,
        _access_token: &str,
        _chatgpt_account_id: &str,
        _chatgpt_plan_type: Option<&str>,
    ) -> std::io::Result<()> {
        Ok(())
    }

    pub fn load_auth_dot_json(
        codex_home: &std::path::Path,
    ) -> std::io::Result<Option<super::AuthDotJson>> {
        let path = codex_home.join("auth.json");
        match std::fs::read_to_string(&path) {
            Ok(contents) => {
                eprintln!(
                    "[wasi-codex-login] loaded auth.json from {}",
                    path.display()
                );
                match serde_json::from_str(&contents) {
                    Ok(auth) => Ok(Some(auth)),
                    Err(e) => {
                        eprintln!("[wasi-codex-login] failed to parse auth.json: {e}");
                        Ok(None)
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => {
                eprintln!("[wasi-codex-login] failed to read auth.json: {e}");
                Ok(None)
            }
        }
    }

    pub fn enforce_login_restrictions(_config: &AuthConfig) -> std::io::Result<()> {
        Ok(())
    }

    pub fn login_with_api_key(
        codex_home: &std::path::Path,
        api_key: &str,
        _auth_credentials_store_mode: AuthCredentialsStoreMode,
    ) -> std::io::Result<()> {
        let auth_json = super::AuthDotJson {
            auth_mode: Some(super::AuthMode::ApiKey),
            openai_api_key: Some(api_key.to_string()),
            tokens: None,
            last_refresh: None,
        };
        save_auth(codex_home, &auth_json, _auth_credentials_store_mode)
    }

    pub fn logout(
        _codex_home: &std::path::Path,
        _auth_credentials_store_mode: AuthCredentialsStoreMode,
    ) -> std::io::Result<bool> {
        Ok(false)
    }

    pub fn save_auth(
        codex_home: &std::path::Path,
        auth_dot_json: &super::AuthDotJson,
        _auth_credentials_store_mode: AuthCredentialsStoreMode,
    ) -> std::io::Result<()> {
        std::fs::create_dir_all(codex_home)?;
        let path = codex_home.join("auth.json");
        let json = serde_json::to_string_pretty(auth_dot_json)
            .map_err(|e| std::io::Error::other(format!("serialize auth.json: {e}")))?;
        eprintln!("[wasi-codex-login] saving auth.json to {}", path.display());
        std::fs::write(&path, json)?;
        Ok(())
    }

    pub mod default_client {
        pub use super::super::default_client::*;
    }
}

// ---------------------------------------------------------------------------
// AuthCredentialsStoreMode
// ---------------------------------------------------------------------------

#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum AuthCredentialsStoreMode {
    #[default]
    File,
    Keyring,
    Auto,
    Ephemeral,
}

// ---------------------------------------------------------------------------
// AuthDotJson (stub)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuthDotJson {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_mode: Option<AuthMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_refresh: Option<String>,
}

// ---------------------------------------------------------------------------
// Token data module
// ---------------------------------------------------------------------------

pub mod token_data {
    use serde::{Deserialize, Serialize};

    /// Flat subset of useful claims in id_token from auth.json.
    #[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
    pub struct IdTokenInfo {
        pub email: Option<String>,
        pub chatgpt_plan_type: Option<PlanType>,
        pub chatgpt_user_id: Option<String>,
        pub chatgpt_account_id: Option<String>,
        pub raw_jwt: String,
    }

    impl IdTokenInfo {
        pub fn is_workspace_account(&self) -> bool {
            false
        }
    }

    #[derive(Clone, Debug, Default, Serialize, Deserialize)]
    pub struct TokenData {
        pub id_token: IdTokenInfo,
        pub access_token: String,
        pub refresh_token: String,
        pub account_id: Option<String>,
        pub organization_id: Option<String>,
        pub project_id: Option<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(untagged)]
    pub enum PlanType {
        Known(KnownPlan),
        Unknown(String),
    }

    impl PlanType {
        pub fn from_raw_value(raw: &str) -> Self {
            match raw.to_ascii_lowercase().as_str() {
                "free" => Self::Known(KnownPlan::Free),
                "go" => Self::Known(KnownPlan::Go),
                "plus" => Self::Known(KnownPlan::Plus),
                "pro" => Self::Known(KnownPlan::Pro),
                "team" => Self::Known(KnownPlan::Team),
                "business" => Self::Known(KnownPlan::Business),
                "enterprise" => Self::Known(KnownPlan::Enterprise),
                "education" | "edu" => Self::Known(KnownPlan::Edu),
                _ => Self::Unknown(raw.to_string()),
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum KnownPlan {
        Free,
        Go,
        Plus,
        Pro,
        Team,
        Business,
        Enterprise,
        Edu,
    }

    /// Decode token data from a JWT string (base64 payload, no signature verification).
    pub fn decode_token_data(token: &str) -> Option<TokenData> {
        decode_jwt_payload(token)
    }

    /// Decode token data from the saved `tokens` JSON object in auth.json.
    pub fn decode_token_data_from_json(tokens: &serde_json::Value) -> Option<TokenData> {
        let access_token = tokens.get("access_token")?.as_str()?.to_string();
        let refresh_token = tokens
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let id_token_raw = tokens.get("id_token").and_then(|v| v.as_str());

        let id_token = id_token_raw
            .and_then(|jwt| {
                let claims = decode_jwt_claims(jwt)?;
                Some(IdTokenInfo {
                    email: claims.get("email").and_then(|v| v.as_str()).map(String::from),
                    chatgpt_plan_type: claims
                        .get("https://api.openai.com/plan_type")
                        .and_then(|v| v.as_str())
                        .map(PlanType::from_raw_value),
                    chatgpt_user_id: claims
                        .get("https://api.openai.com/auth")
                        .and_then(|v| v.get("user_id"))
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    chatgpt_account_id: claims.get("sub").and_then(|v| v.as_str()).map(String::from),
                    raw_jwt: jwt.to_string(),
                })
            })
            .unwrap_or_default();

        let account_id = id_token.chatgpt_account_id.clone();

        Some(TokenData {
            id_token,
            access_token,
            refresh_token,
            account_id,
            organization_id: None,
            project_id: None,
        })
    }

    /// Decode the payload of a JWT without signature verification.
    /// JWTs are `header.payload.signature` — we base64-decode the middle part.
    fn decode_jwt_claims(jwt: &str) -> Option<serde_json::Value> {
        use base64::Engine;
        let parts: Vec<&str> = jwt.splitn(3, '.').collect();
        if parts.len() < 2 {
            return None;
        }
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[1])
            .ok()?;
        serde_json::from_slice(&payload).ok()
    }

    fn decode_jwt_payload(jwt: &str) -> Option<TokenData> {
        let claims = decode_jwt_claims(jwt)?;
        let email = claims.get("email").and_then(|v| v.as_str()).map(String::from);
        let account_id = claims.get("sub").and_then(|v| v.as_str()).map(String::from);

        Some(TokenData {
            id_token: IdTokenInfo {
                email,
                chatgpt_account_id: account_id.clone(),
                ..Default::default()
            },
            access_token: jwt.to_string(),
            refresh_token: String::new(),
            account_id,
            organization_id: None,
            project_id: None,
        })
    }
}

pub use token_data::IdTokenInfo;
pub use token_data::TokenData;

// Re-export auth types at the top level (upstream `codex_login` does this)
pub use auth::ExternalAuthRefreshContext;
pub use auth::ExternalAuthRefreshReason;
pub use auth::ExternalAuthRefresher;
pub use auth::ExternalAuthTokens;
pub use auth::RefreshTokenError;
pub use auth::RefreshTokenFailedError;
pub use auth::RefreshTokenFailedReason;
pub use auth::UnauthorizedRecovery;
pub use auth::UnauthorizedRecoveryStepResult;
pub use auth::enforce_login_restrictions;
pub use auth::load_auth_dot_json;
pub use auth::login_with_api_key;
pub use auth::login_with_chatgpt_auth_tokens;
pub use auth::logout;
pub use auth::save_auth;

// ---------------------------------------------------------------------------
// ForcedLoginMethod (stub for codex_protocol::config_types::ForcedLoginMethod)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForcedLoginMethod {
    Chatgpt,
    Api,
}

impl std::fmt::Display for ForcedLoginMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Chatgpt => write!(f, "chatgpt"),
            Self::Api => write!(f, "api"),
        }
    }
}

// ---------------------------------------------------------------------------
// AuthConfig (stub for codex_login::auth::AuthConfig)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthConfig {
    pub codex_home: PathBuf,
    pub auth_credentials_store_mode: AuthCredentialsStoreMode,
    pub forced_login_method: Option<ForcedLoginMethod>,
    pub forced_chatgpt_workspace_id: Option<String>,
}

// ---------------------------------------------------------------------------
// DeviceCode (stub for device_code_auth::DeviceCode)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DeviceCode {
    pub verification_url: String,
    pub user_code: String,
    device_auth_id: String,
    interval: u64,
}

// ---------------------------------------------------------------------------
// ServerOptions (stub for server::ServerOptions)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ServerOptions {
    pub codex_home: PathBuf,
    pub client_id: String,
    pub issuer: String,
    pub port: u16,
    pub open_browser: bool,
    pub force_state: Option<String>,
    pub forced_chatgpt_workspace_id: Option<String>,
    pub cli_auth_credentials_store_mode: AuthCredentialsStoreMode,
}

impl ServerOptions {
    pub fn new(
        codex_home: PathBuf,
        client_id: String,
        forced_chatgpt_workspace_id: Option<String>,
        cli_auth_credentials_store_mode: AuthCredentialsStoreMode,
    ) -> Self {
        Self {
            codex_home,
            client_id,
            issuer: DEFAULT_ISSUER.to_string(),
            port: 0,
            open_browser: true,
            force_state: None,
            forced_chatgpt_workspace_id,
            cli_auth_credentials_store_mode,
        }
    }
}

// ---------------------------------------------------------------------------
// LoginServer (stub)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct LoginServer {
    pub auth_url: String,
    pub actual_port: u16,
    codex_home: Option<std::path::PathBuf>,
}

impl LoginServer {
    /// Block until the OAuth callback completes and auth.json is written.
    /// Polls the filesystem for .login_pending.json removal (the callback
    /// handler deletes it after successfully saving auth.json).
    pub async fn block_until_done(self) -> std::io::Result<()> {
        let home = match &self.codex_home {
            Some(h) => h.clone(),
            None => return Ok(()),
        };
        let pending_path = home.join(".login_pending.json");

        // Poll every 500ms for up to 5 minutes
        for _ in 0..600 {
            // Check if the pending file has been removed (callback completed)
            if !pending_path.exists() {
                // Verify auth.json exists
                if home.join("auth.json").exists() {
                    return Ok(());
                }
            }
            // Sleep 500ms — yield to the async runtime instead of blocking
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        // Timeout — clean up pending file
        let _ = std::fs::remove_file(&pending_path);
        Err(std::io::Error::other("Login timed out after 5 minutes"))
    }

    pub fn cancel_handle(&self) -> ShutdownHandle {
        ShutdownHandle
    }
}

// ---------------------------------------------------------------------------
// ShutdownHandle (stub for server::ShutdownHandle)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ShutdownHandle;

impl ShutdownHandle {
    pub fn shutdown(&self) {}
}

// ---------------------------------------------------------------------------
// Top-level stub functions matching upstream re-exports
// ---------------------------------------------------------------------------

pub async fn request_device_code(_opts: &ServerOptions) -> std::io::Result<DeviceCode> {
    // Return NotFound to trigger fallback to run_login_server() which uses
    // the browser-based OAuth flow via the incoming HTTP handler.
    eprintln!("[wasi-codex-login] request_device_code returning NotFound to trigger browser fallback");
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "device code not available — falling back to browser login",
    ))
}

pub async fn complete_device_code_login(
    _opts: ServerOptions,
    _device_code: DeviceCode,
) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "device code auth not supported in WASM",
    ))
}

pub async fn run_device_code_login(
    _client_id: &str,
    _codex_home: &std::path::Path,
    _auth_credentials_store_mode: AuthCredentialsStoreMode,
) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "device code auth not supported in WASM",
    ))
}

pub fn run_login_server(options: ServerOptions) -> std::io::Result<LoginServer> {
    use base64::Engine;
    use sha2::Digest;

    eprintln!(
        "[wasi-codex-login] run_login_server called, codex_home={}, client_id={}",
        options.codex_home.display(),
        options.client_id
    );

    // Generate PKCE using cryptographic random (matches upstream pkce.rs)
    let mut verifier_bytes = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut verifier_bytes);
    let code_verifier =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(verifier_bytes);
    let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(code_verifier.as_bytes()));

    // Generate state (matches upstream generate_state)
    let mut state_bytes = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut state_bytes);
    let state =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(state_bytes);

    let redirect_uri = format!(
        "{}/oauth-callback",
        std::env::var("CODEX_ORIGIN")
            .unwrap_or_else(|_| "https://agent.edge-agent.dev".into())
    );

    let issuer = if options.issuer.is_empty() {
        DEFAULT_ISSUER
    } else {
        options.issuer.trim_end_matches('/')
    };

    // Write pending login info to OPFS so ts-runtime-mcp's /oauth/callback
    // handler can read it for the token exchange.
    let pending = serde_json::json!({
        "state": state,
        "code_verifier": code_verifier,
        "client_id": options.client_id,
        "redirect_uri": redirect_uri,
        "issuer": issuer,
    });
    let pending_path = options.codex_home.join(".login_pending.json");
    std::fs::create_dir_all(&options.codex_home)?;
    std::fs::write(
        &pending_path,
        serde_json::to_string(&pending).map_err(|e| std::io::Error::other(e.to_string()))?,
    )?;

    // Build the authorization URL (matches upstream build_authorize_url)
    let auth_url = format!(
        "{issuer}/oauth/authorize?\
         response_type=code\
         &client_id={}\
         &redirect_uri={}\
         &scope={}\
         &code_challenge={}\
         &code_challenge_method=S256\
         &id_token_add_organizations=true\
         &codex_cli_simplified_flow=true\
         &state={}\
         &originator=codex_cli_rs",
        urlencoded(&options.client_id),
        urlencoded(&redirect_uri),
        urlencoded(
            "openid profile email offline_access api.connectors.read api.connectors.invoke",
        ),
        urlencoded(&code_challenge),
        urlencoded(&state),
    );

    eprintln!("[wasi-codex-login] auth_url={}", auth_url);

    Ok(LoginServer {
        auth_url,
        actual_port: 0,
        codex_home: Some(options.codex_home),
    })
}

/// Simple percent-encoding for URL query parameters.
fn urlencoded(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            _ => {
                result.push('%');
                result.push_str(&format!("{:02X}", b));
            }
        }
    }
    result
}


// ---------------------------------------------------------------------------
// Default client module
// ---------------------------------------------------------------------------

pub mod default_client {
    use reqwest::header::{HeaderMap, HeaderValue};

    pub use super::AuthManager;
    pub use super::CodexAuth;

    pub const DEFAULT_ORIGINATOR: &str = "codex_cli_rs";
    pub const CODEX_INTERNAL_ORIGINATOR_OVERRIDE_ENV_VAR: &str =
        "CODEX_INTERNAL_ORIGINATOR_OVERRIDE";
    pub const RESIDENCY_HEADER_NAME: &str = "x-openai-internal-codex-residency";

    #[derive(Debug, Clone)]
    pub struct Originator {
        pub value: String,
        pub header_value: HeaderValue,
    }

    #[derive(Debug)]
    pub enum SetOriginatorError {
        InvalidHeaderValue,
        AlreadyInitialized,
    }

    pub fn set_default_originator(_value: String) -> Result<(), SetOriginatorError> {
        Ok(())
    }

    pub fn originator() -> Originator {
        Originator {
            value: DEFAULT_ORIGINATOR.to_string(),
            header_value: HeaderValue::from_static(DEFAULT_ORIGINATOR),
        }
    }

    pub fn is_first_party_originator(originator_value: &str) -> bool {
        originator_value == DEFAULT_ORIGINATOR
            || originator_value == "codex_vscode"
            || originator_value.starts_with("Codex ")
    }

    pub fn is_first_party_chat_originator(originator_value: &str) -> bool {
        originator_value == "codex_atlas" || originator_value == "codex_chatgpt_desktop"
    }

    pub fn get_codex_user_agent() -> String {
        format!("{}/0.0.0 (wasm32-wasip2)", DEFAULT_ORIGINATOR)
    }

    /// Build a default reqwest::Client (our shim).
    pub fn build_reqwest_client() -> reqwest::Client {
        reqwest::Client::new()
    }

    /// Build a reqwest::Client that might fail.
    pub fn try_build_reqwest_client() -> Result<reqwest::Client, std::io::Error> {
        Ok(reqwest::Client::new())
    }

    /// Create the default HTTP client used by Codex.
    pub fn create_client() -> reqwest::Client {
        build_reqwest_client()
    }

    /// Return default HTTP headers for Codex requests.
    pub fn default_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("originator", originator().header_value);
        headers
    }

    pub fn set_default_client_residency_requirement(_enforce_residency: Option<()>) {}
}
