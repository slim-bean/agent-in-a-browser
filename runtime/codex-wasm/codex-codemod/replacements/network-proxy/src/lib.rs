//! Stub module — network-proxy replaced with stubs for wasip2.
#![allow(dead_code, unused_variables, unused_imports)]

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};

// Config types
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkProxyConfig {
    pub network: NetworkProxySettings,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkProxySettings {
    pub enabled: bool,
    pub proxy_url: String,
    pub enable_socks5: bool,
    pub socks_url: String,
    pub enable_socks5_udp: bool,
    pub allow_upstream_proxy: bool,
    pub dangerously_allow_non_loopback_proxy: bool,
    pub dangerously_allow_all_unix_sockets: bool,
    pub mode: NetworkMode,
    pub allowed_domains: Vec<String>,
    pub denied_domains: Vec<String>,
    pub allow_unix_sockets: Vec<String>,
    pub allow_local_binding: bool,
    pub mitm: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkMode { #[default] Limited, Full }

pub struct RuntimeConfig {
    pub http_addr: SocketAddr,
    pub socks_addr: SocketAddr,
}

pub fn host_and_port_from_network_addr(_value: &str, _default_port: u16) -> String { String::new() }
pub fn resolve_runtime(_cfg: &NetworkProxyConfig) -> Result<RuntimeConfig> {
    anyhow::bail!("not available in WASM")
}

// Network policy types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkProtocol { Http, HttpsConnect, Socks5Tcp, Socks5Udp }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkPolicyDecision { Deny, Ask }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkDecisionSource { BaselinePolicy, ModeGuard, ProxyState, Decider }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPolicyRequest {
    pub protocol: NetworkProtocol,
    pub host: String,
    pub port: u16,
    pub client_addr: Option<String>,
    pub method: Option<String>,
    pub command: Option<String>,
    pub exec_policy_hint: Option<String>,
}

pub struct NetworkPolicyRequestArgs {
    pub protocol: NetworkProtocol,
    pub host: String,
    pub port: u16,
    pub client_addr: Option<String>,
    pub method: Option<String>,
    pub command: Option<String>,
    pub exec_policy_hint: Option<String>,
}

impl NetworkPolicyRequest {
    pub fn new(args: NetworkPolicyRequestArgs) -> Self {
        Self { protocol: args.protocol, host: args.host, port: args.port, client_addr: args.client_addr, method: args.method, command: args.command, exec_policy_hint: args.exec_policy_hint }
    }
}

#[derive(Debug, Clone)]
pub enum NetworkDecision {
    Allow,
    Deny { reason: String, source: NetworkDecisionSource, decision: NetworkPolicyDecision },
}

impl NetworkDecision {
    pub fn deny(reason: impl Into<String>) -> Self {
        Self::Deny { reason: reason.into(), source: NetworkDecisionSource::BaselinePolicy, decision: NetworkPolicyDecision::Deny }
    }
    pub fn ask(reason: impl Into<String>) -> Self {
        Self::Deny { reason: reason.into(), source: NetworkDecisionSource::BaselinePolicy, decision: NetworkPolicyDecision::Ask }
    }
    pub fn deny_with_source(reason: impl Into<String>, source: NetworkDecisionSource) -> Self {
        Self::Deny { reason: reason.into(), source, decision: NetworkPolicyDecision::Deny }
    }
    pub fn ask_with_source(reason: impl Into<String>, source: NetworkDecisionSource) -> Self {
        Self::Deny { reason: reason.into(), source, decision: NetworkPolicyDecision::Ask }
    }
}

#[async_trait::async_trait]
pub trait NetworkPolicyDecider: Send + Sync + 'static {
    async fn decide(&self, req: NetworkPolicyRequest) -> NetworkDecision;
}

#[async_trait::async_trait]
impl<F, Fut> NetworkPolicyDecider for F
where
    F: Fn(NetworkPolicyRequest) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = NetworkDecision> + Send,
{
    async fn decide(&self, req: NetworkPolicyRequest) -> NetworkDecision {
        (self)(req).await
    }
}

// Proxy types
pub const PROXY_URL_ENV_KEYS: &[&str] = &["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"];
pub const ALL_PROXY_ENV_KEYS: &[&str] = &["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy", "ALL_PROXY", "all_proxy"];
pub const ALLOW_LOCAL_BINDING_ENV_KEY: &str = "CODEX_ALLOW_LOCAL_BINDING";
pub const NO_PROXY_ENV_KEYS: &[&str] = &["NO_PROXY", "no_proxy"];
pub const DEFAULT_NO_PROXY_VALUE: &str = "";

pub fn proxy_url_env_value<'a>(_env: &'a HashMap<String, String>, _canonical_key: &str) -> Option<&'a str> { None }
pub fn has_proxy_url_env_vars(_env: &HashMap<String, String>) -> bool { false }
pub fn normalize_host(host: &str) -> String { host.to_string() }

pub struct NetworkProxyBuilder;
#[derive(Clone, Debug)]
pub struct NetworkProxy;

impl Default for NetworkProxyBuilder {
    fn default() -> Self { Self }
}

impl NetworkProxyBuilder {
    pub fn state(self, _state: Arc<NetworkProxyState>) -> Self { self }
    pub fn http_addr(self, _addr: SocketAddr) -> Self { self }
    pub fn socks_addr(self, _addr: SocketAddr) -> Self { self }
    pub fn managed_by_codex(self, _managed: bool) -> Self { self }
    pub fn policy_decider<D: NetworkPolicyDecider>(self, _decider: D) -> Self { self }
    pub fn policy_decider_arc(self, _decider: Arc<dyn NetworkPolicyDecider>) -> Self { self }
    pub fn blocked_request_observer<O: BlockedRequestObserver>(self, _observer: O) -> Self { self }
    pub fn blocked_request_observer_arc(self, _observer: Arc<dyn BlockedRequestObserver>) -> Self { self }
    pub async fn build(self) -> Result<NetworkProxy> { anyhow::bail!("not available in WASM") }
}

impl NetworkProxy {
    pub fn builder() -> NetworkProxyBuilder { NetworkProxyBuilder }
    pub fn apply_to_env(&self, _env: &mut HashMap<String, String>) {}
    pub fn http_addr(&self) -> SocketAddr { SocketAddr::from(([127, 0, 0, 1], 0)) }
    pub fn socks_addr(&self) -> SocketAddr { SocketAddr::from(([127, 0, 0, 1], 0)) }
    pub async fn current_cfg(&self) -> Result<NetworkProxyConfig> { Ok(NetworkProxyConfig::default()) }
    pub async fn add_allowed_domain(&self, _host: &str) -> Result<()> { Ok(()) }
    pub async fn add_denied_domain(&self, _host: &str) -> Result<()> { Ok(()) }
    pub fn allow_local_binding(&self) -> bool { false }
    pub fn allow_unix_sockets(&self) -> &[String] { &[] }
    pub fn dangerously_allow_all_unix_sockets(&self) -> bool { false }
    pub async fn run(&self) -> Result<NetworkProxyHandle> { Ok(NetworkProxyHandle) }
}

#[derive(Debug)]
pub struct NetworkProxyHandle;

impl NetworkProxyHandle {
    pub fn noop() -> Self { Self }
}

// Runtime/state types
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkProxyAuditMetadata {
    pub conversation_id: Option<String>,
    pub app_version: Option<String>,
    pub user_account_id: Option<String>,
    pub auth_mode: Option<String>,
    pub originator: Option<String>,
    pub user_email: Option<String>,
    pub terminal_type: Option<String>,
    pub model: Option<String>,
    pub slug: Option<String>,
}

#[derive(Debug, Clone)]
pub enum HostBlockReason { Denied, NotAllowed, NotAllowedLocal }

#[derive(Debug, Clone)]
pub enum HostBlockDecision { Allowed, Blocked(HostBlockReason) }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedRequest {
    pub host: String,
    pub reason: String,
    pub client: Option<String>,
    pub method: Option<String>,
    pub mode: Option<NetworkMode>,
    pub protocol: String,
    pub decision: Option<String>,
    pub source: Option<String>,
    pub port: Option<u16>,
    pub timestamp: i64,
}

pub struct BlockedRequestArgs {
    pub host: String,
    pub reason: String,
    pub client: Option<String>,
    pub method: Option<String>,
    pub mode: Option<NetworkMode>,
    pub protocol: String,
    pub decision: Option<String>,
    pub source: Option<String>,
    pub port: Option<u16>,
}

impl BlockedRequest {
    pub fn new(args: BlockedRequestArgs) -> Self {
        Self { host: args.host, reason: args.reason, client: args.client, method: args.method, mode: args.mode, protocol: args.protocol, decision: args.decision, source: args.source, port: args.port, timestamp: 0 }
    }
}

#[derive(Clone)]
pub struct ConfigState {
    pub config: NetworkProxyConfig,
    pub constraints: NetworkProxyConstraints,
    pub blocked: VecDeque<BlockedRequest>,
    pub blocked_total: u64,
}

#[async_trait::async_trait]
pub trait ConfigReloader: Send + Sync {
    fn source_label(&self) -> String;
    async fn maybe_reload(&self) -> Result<Option<ConfigState>>;
    async fn reload_now(&self) -> Result<ConfigState>;
}

#[async_trait::async_trait]
pub trait BlockedRequestObserver: Send + Sync + 'static {
    async fn on_blocked_request(&self, request: BlockedRequest);
}

#[async_trait::async_trait]
impl<F, Fut> BlockedRequestObserver for F
where
    F: Fn(BlockedRequest) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    async fn on_blocked_request(&self, request: BlockedRequest) {
        (self)(request).await
    }
}

pub struct NetworkProxyState;

impl NetworkProxyState {
    pub fn with_reloader(_state: ConfigState, _reloader: Arc<dyn ConfigReloader>) -> Self { Self }
    pub fn with_reloader_and_blocked_observer(_state: ConfigState, _reloader: Arc<dyn ConfigReloader>, _observer: Option<Arc<dyn BlockedRequestObserver>>) -> Self { Self }
    pub fn with_reloader_and_audit_metadata(_state: ConfigState, _reloader: Arc<dyn ConfigReloader>, _audit_metadata: NetworkProxyAuditMetadata) -> Self { Self }
    pub fn with_reloader_and_audit_metadata_and_blocked_observer(_state: ConfigState, _reloader: Arc<dyn ConfigReloader>, _audit_metadata: NetworkProxyAuditMetadata, _observer: Option<Arc<dyn BlockedRequestObserver>>) -> Self { Self }
    pub async fn set_blocked_request_observer(&self, _observer: Option<Arc<dyn BlockedRequestObserver>>) {}
    pub fn audit_metadata(&self) -> &NetworkProxyAuditMetadata { &EMPTY_AUDIT }
    pub async fn current_cfg(&self) -> Result<NetworkProxyConfig> { Ok(NetworkProxyConfig::default()) }
    pub async fn current_patterns(&self) -> Result<(Vec<String>, Vec<String>)> { Ok((vec![], vec![])) }
    pub async fn enabled(&self) -> Result<bool> { Ok(false) }
    pub async fn force_reload(&self) -> Result<()> { Ok(()) }
    pub async fn host_blocked(&self, _host: &str, _port: u16) -> Result<HostBlockDecision> { Ok(HostBlockDecision::Allowed) }
}

static EMPTY_AUDIT: NetworkProxyAuditMetadata = NetworkProxyAuditMetadata {
    conversation_id: None, app_version: None, user_account_id: None, auth_mode: None,
    originator: None, user_email: None, terminal_type: None, model: None, slug: None,
};

// State types
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkProxyConstraints {
    pub enabled: Option<bool>,
    pub mode: Option<NetworkMode>,
    pub allow_upstream_proxy: Option<bool>,
    pub dangerously_allow_non_loopback_proxy: Option<bool>,
    pub dangerously_allow_all_unix_sockets: Option<bool>,
    pub allowed_domains: Option<Vec<String>>,
    pub allowlist_expansion_enabled: Option<bool>,
    pub denied_domains: Option<Vec<String>>,
    pub denylist_expansion_enabled: Option<bool>,
    pub allow_unix_sockets: Option<Vec<String>>,
    pub allow_local_binding: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PartialNetworkProxyConfig {
    pub network: PartialNetworkConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PartialNetworkConfig {
    pub enabled: Option<bool>,
    pub mode: Option<NetworkMode>,
    pub allow_upstream_proxy: Option<bool>,
    pub dangerously_allow_non_loopback_proxy: Option<bool>,
    pub dangerously_allow_all_unix_sockets: Option<bool>,
    pub allowed_domains: Option<Vec<String>>,
    pub denied_domains: Option<Vec<String>>,
    pub allow_unix_sockets: Option<Vec<String>>,
    pub allow_local_binding: Option<bool>,
}

pub fn build_config_state(config: NetworkProxyConfig, constraints: NetworkProxyConstraints) -> Result<ConfigState> {
    Ok(ConfigState { config, constraints, blocked: VecDeque::new(), blocked_total: 0 })
}

#[derive(Debug)]
pub struct NetworkProxyConstraintError(pub String);
impl std::fmt::Display for NetworkProxyConstraintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) }
}
impl std::error::Error for NetworkProxyConstraintError {}
impl NetworkProxyConstraintError {
    pub fn into_anyhow(self) -> anyhow::Error { anyhow::anyhow!("{}", self.0) }
}

pub fn validate_policy_against_constraints(_config: &NetworkProxyConfig, _constraints: &NetworkProxyConstraints) -> std::result::Result<(), NetworkProxyConstraintError> { Ok(()) }
