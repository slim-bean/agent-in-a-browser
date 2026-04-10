#![allow(dead_code, unused_variables, unused_imports)]
//! Stub for codex-otel in wasip2 — no-op telemetry.

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

/// Session telemetry — no-op in WASM.
#[derive(Clone, Debug, Default)]
pub struct SessionTelemetry;

impl SessionTelemetry {
    pub fn new(
        _conversation_id: impl std::fmt::Display,
        _model: impl std::fmt::Display,
        _slug: impl std::fmt::Display,
        _account_id: impl std::fmt::Debug,
        _account_email: impl std::fmt::Debug,
        _auth_mode: impl std::fmt::Debug,
        _originator: impl std::fmt::Display,
        _log_user_prompts: bool,
        _terminal_type: impl std::fmt::Display,
        _session_source: impl std::fmt::Display,
    ) -> Self {
        Self
    }

    pub fn record_feature_flags(&self, _flags: &HashMap<String, bool>) {}

    pub fn record_feature_flag(&self, _name: &str, _value: bool) {}

    pub fn counter(&self, _name: &str, _inc: i64, _tags: &[(&str, &str)]) {}

    pub fn histogram(&self, _name: &str, _value: i64, _tags: &[(&str, &str)]) {}

    pub fn gauge(&self, _name: &str, _value: f64, _attrs: &[(&str, &str)]) {}

    pub fn record_duration(&self, _name: &str, _duration: Duration, _tags: &[(&str, &str)]) {}

    pub fn start_timer(&self, _name: &str, _tags: &[(&str, &str)]) -> Result<Timer, MetricsError> {
        Err(MetricsError::ExporterDisabled)
    }

    pub fn shutdown_metrics(&self) -> MetricsResult<()> {
        Ok(())
    }

    pub fn snapshot_metrics(&self) -> MetricsResult<ResourceMetrics> {
        Err(MetricsError::ExporterDisabled)
    }

    pub fn reset_runtime_metrics(&self) {}

    #[allow(clippy::too_many_arguments)]
    pub fn record_auth_recovery(
        &self,
        _mode: &str,
        _step: &str,
        _outcome: &str,
        _request_id: Option<&str>,
        _cf_ray: Option<&str>,
        _auth_error: Option<&str>,
        _auth_error_code: Option<&str>,
        _recovery_reason: Option<&str>,
        _auth_state_changed: Option<bool>,
    ) {
    }

    pub fn tool_decision(
        &self,
        _tool_name: &str,
        _call_id: &str,
        _decision: &impl std::fmt::Display,
        _source: ToolDecisionSource,
    ) {
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn log_tool_result_with_tags<F, Fut, E>(
        &self,
        _tool_name: &str,
        _call_id: &str,
        _arguments: &str,
        _extra_tags: &[(&str, &str)],
        _mcp_server: Option<&str>,
        _mcp_server_origin: Option<&str>,
        f: F,
    ) -> Result<(String, bool), E>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(String, bool), E>>,
        E: std::fmt::Display,
    {
        f().await
    }

    #[allow(clippy::too_many_arguments)]
    pub fn tool_result_with_tags(
        &self,
        _tool_name: &str,
        _call_id: &str,
        _arguments: &str,
        _duration: Duration,
        _success: bool,
        _output: &str,
        _extra_tags: &[(&str, &str)],
        _mcp_server: Option<&str>,
        _mcp_server_origin: Option<&str>,
    ) {
    }

    pub fn log_tool_failed(&self, _tool_name: &str, _error: &str) {}

    pub fn with_model(self, _model: &str, _slug: &str) -> Self {
        Self
    }

    pub fn with_auth_env(self, _auth_env: AuthEnvTelemetryMetadata) -> Self {
        Self
    }

    pub fn runtime_metrics_summary(&self) -> Option<RuntimeMetricsSummary> {
        None
    }

    pub fn with_metrics_service_name(self, _service_name: &str) -> Self {
        Self
    }

    pub fn with_metrics(self, _metrics: metrics::MetricsClient) -> Self {
        Self
    }

    pub fn with_metrics_without_metadata_tags(self, _metrics: metrics::MetricsClient) -> Self {
        Self
    }

    pub fn with_provider_metrics(self, _provider: &OtelProvider) -> Self {
        Self
    }

    #[allow(clippy::too_many_arguments)]
    pub fn conversation_starts(
        &self,
        _provider_name: &str,
        _reasoning_effort: impl std::fmt::Debug,
        _reasoning_summary: impl std::fmt::Debug,
        _context_window: Option<i64>,
        _auto_compact_limit: Option<i64>,
        _approval_policy: impl std::fmt::Debug,
        _sandbox_policy: impl std::fmt::Debug,
        _mcp_servers: Vec<&str>,
        _active_profile: impl std::fmt::Debug,
    ) {
    }

    pub fn user_prompt<T>(&self, _items: &[T]) {}

    #[allow(clippy::too_many_arguments)]
    pub fn record_api_request(
        &self,
        _attempt: u64,
        _status: Option<u16>,
        _error: Option<&str>,
        _duration: Duration,
        _auth_header_attached: bool,
        _auth_header_name: Option<&str>,
        _retry_after_unauthorized: bool,
        _recovery_mode: Option<&str>,
        _recovery_phase: Option<&str>,
        _endpoint: &str,
        _request_id: Option<&str>,
        _cf_ray: Option<&str>,
        _auth_error: Option<&str>,
        _auth_error_code: Option<&str>,
    ) {
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_websocket_connect(
        &self,
        _duration: Duration,
        _status: Option<u16>,
        _error: Option<&str>,
        _auth_header_attached: bool,
        _auth_header_name: Option<&str>,
        _retry_after_unauthorized: bool,
        _recovery_mode: Option<&str>,
        _recovery_phase: Option<&str>,
        _endpoint: &str,
        _connection_reused: bool,
        _request_id: Option<&str>,
        _cf_ray: Option<&str>,
        _auth_error: Option<&str>,
        _auth_error_code: Option<&str>,
    ) {
    }

    pub fn record_responses<T, E>(&self, _handle: &T, _event: &E) {}

    pub fn record_websocket_event<T>(&self, _result: T, _duration: Duration) {}

    pub fn record_websocket_request(
        &self,
        _duration: Duration,
        _error: Option<&str>,
        _connection_reused: bool,
    ) {
    }

    pub fn log_sse_event<T, E1, E2>(
        &self,
        _result: &Result<Option<Result<T, E1>>, E2>,
        _duration: Duration,
    ) where
        E1: std::fmt::Display,
    {
    }

    pub fn sse_event_completed(
        &self,
        _input_token_count: i64,
        _output_token_count: i64,
        _cached_token_count: Option<i64>,
        _reasoning_token_count: Option<i64>,
        _tool_token_count: i64,
    ) {
    }

    pub fn see_event_completed_failed<T>(&self, _error: &T)
    where
        T: std::fmt::Display,
    {
    }

    pub fn on_request(
        &self,
        _attempt: u64,
        _status: Option<u16>,
        _error: Option<&dyn std::fmt::Display>,
        _duration: Duration,
    ) {
    }
}

/// Telemetry auth mode.
#[derive(Clone, Debug, Default)]
pub enum TelemetryAuthMode {
    #[default]
    None,
    ApiKey,
    ChatGpt,
    Chatgpt,
}

impl std::fmt::Display for TelemetryAuthMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::ApiKey => write!(f, "ApiKey"),
            Self::ChatGpt | Self::Chatgpt => write!(f, "Chatgpt"),
        }
    }
}

impl TelemetryAuthMode {
    /// Convert from a Display-able auth mode (e.g. codex_login::AuthMode).
    pub fn from_display(mode: &impl std::fmt::Display) -> Self {
        match mode.to_string().as_str() {
            "apikey" => Self::ApiKey,
            "chatgpt" | "chatgptAuthTokens" => Self::ChatGpt,
            _ => Self::None,
        }
    }
}

/// Auth environment telemetry metadata.
#[derive(Clone, Debug, Default)]
pub struct AuthEnvTelemetryMetadata {
    pub openai_api_key_env_present: bool,
    pub codex_api_key_env_present: bool,
    pub codex_api_key_env_enabled: bool,
    pub provider_env_key_name: Option<String>,
    pub provider_env_key_present: Option<bool>,
    pub refresh_token_url_override_present: bool,
}

/// Tool decision source.
#[derive(Clone, Debug)]
pub enum ToolDecisionSource {
    AutomatedReviewer,
    Config,
    User,
}

impl std::fmt::Display for ToolDecisionSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AutomatedReviewer => write!(f, "automated_reviewer"),
            Self::Config => write!(f, "config"),
            Self::User => write!(f, "user"),
        }
    }
}

/// OtelProvider — no-op in WASM.
pub struct OtelProvider;

impl OtelProvider {
    pub fn shutdown(&self) {}

    pub fn from(
        _settings: &config::OtelSettings,
    ) -> Result<Option<Self>, Box<dyn std::error::Error>> {
        Ok(None)
    }

    pub fn logger_layer(&self) -> Option<NoopLayer> {
        None
    }

    pub fn tracing_layer(&self) -> Option<NoopLayer> {
        None
    }
}

/// Placeholder layer type for tracing — never actually constructed.
pub struct NoopLayer;

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for NoopLayer {}

/// Timer — no-op in WASM.
#[derive(Clone, Debug, Default)]
pub struct Timer;

impl Timer {
    pub fn new() -> Self {
        Self
    }

    pub fn elapsed_ms(&self) -> u64 {
        0
    }

    pub fn elapsed_secs(&self) -> f64 {
        0.0
    }

    pub fn record(&self, _additional_tags: &[(&str, &str)]) -> MetricsResult<()> {
        Ok(())
    }
}

/// Start a metrics timer using the globally installed metrics client — no-op in WASM.
pub fn start_global_timer(_name: &str, _tags: &[(&str, &str)]) -> MetricsResult<Timer> {
    Err(MetricsError::ExporterDisabled)
}

/// Get W3C trace context from current span — no-op in WASM.
/// Returns Option<T> to be generic over the concrete W3cTraceContext type
/// from codex_protocol (avoids a conflicting local type definition).
pub fn current_span_w3c_trace_context<T>() -> Option<T> {
    None
}

/// Get W3C trace context from a specific span — no-op in WASM.
pub fn span_w3c_trace_context<T>(_span: &tracing::Span) -> Option<T> {
    None
}

/// Get trace ID from current span — no-op in WASM.
pub fn current_span_trace_id() -> Option<String> {
    None
}

/// Set parent from W3C trace context — no-op.
pub fn set_parent_from_w3c_trace_context<S, T>(_span: &S, _ctx: &T) -> bool {
    false
}

/// Get context from W3C trace context — returns nothing meaningful.
/// Takes any type by reference — always returns None in WASM.
pub fn context_from_w3c_trace_context<T>(_ctx: &T) -> Option<()> {
    None
}

/// Sanitize a tag value to comply with metric tag validation rules — no-op stub.
pub fn sanitize_metric_tag_value(value: &str) -> String {
    const MAX_LEN: usize = 256;
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = sanitized.trim_matches('_');
    if trimmed.is_empty() || trimmed.chars().all(|ch| !ch.is_ascii_alphanumeric()) {
        return "unspecified".to_string();
    }
    if trimmed.len() <= MAX_LEN {
        trimmed.to_string()
    } else {
        trimmed[..MAX_LEN].to_string()
    }
}

/// Metrics error — stub.
#[derive(Debug)]
pub enum MetricsError {
    ExporterDisabled,
}

impl std::fmt::Display for MetricsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExporterDisabled => write!(f, "metrics exporter is disabled"),
        }
    }
}

impl std::error::Error for MetricsError {}

/// Metrics result type alias.
pub type MetricsResult<T> = Result<T, MetricsError>;

/// Resource metrics snapshot — stub.
#[derive(Debug, Default)]
pub struct ResourceMetrics;

/// Runtime metric totals — stub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuntimeMetricTotals {
    pub count: u64,
    pub duration_ms: u64,
}

impl RuntimeMetricTotals {
    pub fn is_empty(self) -> bool {
        self.count == 0 && self.duration_ms == 0
    }

    pub fn merge(&mut self, other: Self) {
        self.count = self.count.saturating_add(other.count);
        self.duration_ms = self.duration_ms.saturating_add(other.duration_ms);
    }
}

/// Runtime metrics summary — stub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuntimeMetricsSummary {
    pub tool_calls: RuntimeMetricTotals,
    pub api_calls: RuntimeMetricTotals,
    pub streaming_events: RuntimeMetricTotals,
    pub websocket_calls: RuntimeMetricTotals,
    pub websocket_events: RuntimeMetricTotals,
    pub responses_api_overhead_ms: u64,
    pub responses_api_inference_time_ms: u64,
    pub responses_api_engine_iapi_ttft_ms: u64,
    pub responses_api_engine_service_ttft_ms: u64,
    pub responses_api_engine_iapi_tbt_ms: u64,
    pub responses_api_engine_service_tbt_ms: u64,
    pub turn_ttft_ms: u64,
    pub turn_ttfm_ms: u64,
}

/// Responses API summary — stub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResponsesApiSummary {
    pub overhead_ms: u64,
    pub inference_time_ms: u64,
}

impl RuntimeMetricsSummary {
    pub fn merge(&mut self, other: Self) {
        self.tool_calls.merge(other.tool_calls);
        self.api_calls.merge(other.api_calls);
        self.streaming_events.merge(other.streaming_events);
        self.websocket_calls.merge(other.websocket_calls);
        self.websocket_events.merge(other.websocket_events);
        self.responses_api_overhead_ms = self
            .responses_api_overhead_ms
            .saturating_add(other.responses_api_overhead_ms);
        self.responses_api_inference_time_ms = self
            .responses_api_inference_time_ms
            .saturating_add(other.responses_api_inference_time_ms);
    }

    pub fn responses_api_summary(&self) -> Self {
        *self
    }

    pub fn is_empty(self) -> bool {
        self.tool_calls.is_empty()
            && self.api_calls.is_empty()
            && self.streaming_events.is_empty()
            && self.websocket_calls.is_empty()
            && self.websocket_events.is_empty()
            && self.responses_api_overhead_ms == 0
            && self.responses_api_inference_time_ms == 0
            && self.responses_api_engine_iapi_ttft_ms == 0
            && self.responses_api_engine_service_ttft_ms == 0
            && self.responses_api_engine_iapi_tbt_ms == 0
            && self.responses_api_engine_service_tbt_ms == 0
            && self.turn_ttft_ms == 0
            && self.turn_ttfm_ms == 0
    }
}

/// Configuration types.
pub mod config {
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[derive(Clone, Debug)]
    pub struct OtelSettings {
        pub environment: String,
        pub service_name: String,
        pub service_version: String,
        pub codex_home: PathBuf,
        pub exporter: OtelExporter,
        pub trace_exporter: OtelExporter,
        pub metrics_exporter: OtelExporter,
        pub runtime_metrics: bool,
    }

    #[derive(Clone, Debug)]
    pub enum OtelExporter {
        None,
        Statsig,
        OtlpGrpc {
            endpoint: String,
            headers: HashMap<String, String>,
            tls: Option<OtelTlsConfig>,
        },
        OtlpHttp {
            endpoint: String,
            headers: HashMap<String, String>,
            protocol: OtelHttpProtocol,
            tls: Option<OtelTlsConfig>,
        },
    }

    #[derive(Clone, Debug)]
    pub enum OtelHttpProtocol {
        Binary,
        Json,
    }

    #[derive(Clone, Debug, Default)]
    pub struct OtelTlsConfig {
        pub ca_certificate: Option<std::path::PathBuf>,
        pub client_certificate: Option<std::path::PathBuf>,
        pub client_private_key: Option<std::path::PathBuf>,
    }
}

/// Metrics module — no-op stubs.
/// Re-export all metrics types at crate root (matches upstream `pub use crate::metrics::*;`).
pub use metrics::*;
pub mod metrics {
    use std::time::Duration;

    use super::{MetricsError, MetricsResult, ResourceMetrics, Timer};

    /// Metrics client — no-op.
    #[derive(Clone, Debug, Default)]
    pub struct MetricsClient;

    impl MetricsClient {
        pub fn new() -> Self {
            Self
        }

        pub fn counter(&self, _name: &str, _inc: i64, _tags: &[(&str, &str)]) -> MetricsResult<()> {
            Ok(())
        }

        pub fn histogram(
            &self,
            _name: &str,
            _value: i64,
            _tags: &[(&str, &str)],
        ) -> MetricsResult<()> {
            Ok(())
        }

        pub fn record_duration(
            &self,
            _name: &str,
            _duration: Duration,
            _tags: &[(&str, &str)],
        ) -> MetricsResult<()> {
            Ok(())
        }

        pub fn start_timer(
            &self,
            _name: &str,
            _tags: &[(&str, &str)],
        ) -> Result<Timer, MetricsError> {
            Ok(Timer)
        }

        pub fn snapshot(&self) -> MetricsResult<ResourceMetrics> {
            Err(MetricsError::ExporterDisabled)
        }

        pub fn increment_counter(&self, _name: &str, _value: u64) {}

        pub fn record_histogram(&self, _name: &str, _value: f64) {}

        pub fn record_histogram_with_attributes(
            &self,
            _name: &str,
            _value: f64,
            _attrs: &[(&str, String)],
        ) {
        }

        pub fn increment_counter_with_attributes(
            &self,
            _name: &str,
            _value: u64,
            _attrs: &[(&str, String)],
        ) {
        }
    }

    /// Metrics config.
    #[derive(Clone, Debug, Default)]
    pub struct MetricsConfig;

    /// Get global metrics client.
    pub fn global() -> Option<MetricsClient> {
        None
    }

    /// Re-export metric name constants at the metrics module level (matches upstream).
    pub use names::*;

    /// Metric name constants.
    pub mod names {
        pub const TURN_TTFM_DURATION_METRIC: &str = "turn.ttfm.duration";
        pub const TURN_TTFT_DURATION_METRIC: &str = "turn.ttft.duration";
        pub const TURN_E2E_DURATION_METRIC: &str = "turn.e2e.duration";
        pub const TURN_NETWORK_PROXY_METRIC: &str = "turn.network_proxy";
        pub const TURN_TOKEN_USAGE_METRIC: &str = "turn.token_usage";
        pub const TURN_TOOL_CALL_METRIC: &str = "turn.tool_call";
        pub const TOOL_CALL_UNIFIED_EXEC_METRIC: &str = "codex.tool.unified_exec";
        pub const THREAD_STARTED_METRIC: &str = "thread.started";
        pub const STARTUP_PREWARM_DURATION_METRIC: &str = "startup.prewarm.duration";
        pub const STARTUP_PREWARM_AGE_AT_FIRST_TURN_METRIC: &str =
            "startup.prewarm.age_at_first_turn";
    }
}
