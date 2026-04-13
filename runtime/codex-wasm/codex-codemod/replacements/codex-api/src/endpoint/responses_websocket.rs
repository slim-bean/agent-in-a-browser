//! Responses WebSocket for wasip2 — uses browser native WebSocket via WIT backend.
#![allow(dead_code, unused_variables, unused_imports)]
use crate::auth::AuthProvider;
use crate::auth::add_auth_headers_to_header_map;
use crate::common::{ResponseEvent, ResponseStream, ResponsesWsRequest};
use crate::error::ApiError;
use crate::provider::Provider;
use crate::rate_limits::parse_rate_limit_event;
use crate::sse::responses::ResponsesStreamEvent;
use crate::sse::responses::process_responses_event;
use codex_client::TransportError;
use http::HeaderMap;
use http::HeaderName;
use http::HeaderValue;
use http::StatusCode;
use serde::Deserialize;
use serde_json::Value;
use serde_json::map::Map as JsonMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

/// WebSocket stream wrapper backed by the WIT websocket backend.
struct WsStream {
    handle: u32,
}

impl WsStream {
    fn new(handle: u32) -> Self {
        Self { handle }
    }

    fn send_text(&self, text: &str) -> Result<(), ApiError> {
        tokio::websocket_backend::send(self.handle, text)
            .map_err(|e| ApiError::Stream(format!("websocket send failed: {e}")))
    }

    fn recv_text(&self) -> Result<Option<String>, ApiError> {
        tokio::websocket_backend::recv(self.handle)
            .map_err(|e| ApiError::Stream(format!("websocket recv failed: {e}")))
    }

    fn is_closed(&self) -> bool {
        tokio::websocket_backend::is_closed(self.handle)
    }
}

impl Drop for WsStream {
    fn drop(&mut self) {
        tokio::websocket_backend::close(self.handle);
    }
}

const WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE: &str = "websocket_connection_limit_reached";
const WEBSOCKET_CONNECTION_LIMIT_REACHED_MESSAGE: &str =
    "Responses websocket connection limit reached (60 minutes). Create a new websocket connection to continue.";

pub struct ResponsesWebsocketConnection {
    stream: Arc<Mutex<Option<WsStream>>>,
    idle_timeout: Duration,
    server_reasoning_included: bool,
    models_etag: Option<String>,
    server_model: Option<String>,
}

impl std::fmt::Debug for ResponsesWebsocketConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResponsesWebsocketConnection").finish()
    }
}

impl ResponsesWebsocketConnection {
    pub async fn is_closed(&self) -> bool {
        self.stream.lock().await.is_none()
    }

    pub async fn stream_request(
        &self,
        request: ResponsesWsRequest,
        connection_reused: bool,
    ) -> Result<ResponseStream, ApiError> {
        let request_body = serde_json::to_value(&request)
            .map_err(|e| ApiError::Stream(format!("failed to encode ws request: {e}")))?;

        let (tx_event, rx_event) =
            mpsc::channel::<Result<ResponseEvent, ApiError>>(1600);
        let stream = Arc::clone(&self.stream);
        let idle_timeout = self.idle_timeout;
        let server_model = self.server_model.clone();
        let models_etag = self.models_etag.clone();
        let server_reasoning_included = self.server_reasoning_included;

        tokio::spawn(async move {
            if let Some(model) = server_model {
                let _ = tx_event.send(Ok(ResponseEvent::ServerModel(model))).await;
            }
            if let Some(etag) = models_etag {
                let _ = tx_event.send(Ok(ResponseEvent::ModelsEtag(etag))).await;
            }
            if server_reasoning_included {
                let _ = tx_event.send(Ok(ResponseEvent::ServerReasoningIncluded(true))).await;
            }
            let mut guard = stream.lock().await;
            let result = {
                let Some(ws) = guard.as_ref() else {
                    let _ = tx_event.send(Err(ApiError::Stream("websocket closed".into()))).await;
                    return;
                };
                run_ws_response_stream(ws, tx_event.clone(), request_body, idle_timeout).await
            };
            if let Err(err) = result {
                let failed = guard.take();
                drop(guard);
                drop(failed);
                let _ = tx_event.send(Err(err)).await;
            }
        });

        Ok(ResponseStream { rx_event })
    }
}

async fn run_ws_response_stream(
    ws: &WsStream,
    tx_event: mpsc::Sender<Result<ResponseEvent, ApiError>>,
    request_body: Value,
    _idle_timeout: Duration,
) -> Result<(), ApiError> {
    let request_text = serde_json::to_string(&request_body)
        .map_err(|e| ApiError::Stream(format!("failed to encode ws request: {e}")))?;

    ws.send_text(&request_text)?;

    let mut last_server_model: Option<String> = None;
    loop {
        let msg = ws.recv_text()?;
        let Some(text) = msg else {
            return Err(ApiError::Stream("stream closed before response.completed".into()));
        };

        // Check for wrapped error events
        if let Some(wrapped_error) = parse_wrapped_websocket_error_event(&text) {
            if let Some(error) = map_wrapped_websocket_error_event(wrapped_error, text.clone()) {
                return Err(error);
            }
        }

        let event = match serde_json::from_str::<ResponsesStreamEvent>(&text) {
            Ok(event) => event,
            Err(err) => {
                debug!("failed to parse websocket event: {err}, data: {text}");
                continue;
            }
        };

        if event.kind() == "codex.rate_limits" {
            if let Some(snapshot) = parse_rate_limit_event(&text) {
                let _ = tx_event.send(Ok(ResponseEvent::RateLimits(snapshot))).await;
            }
            continue;
        }

        if let Some(model) = event.response_model() {
            if last_server_model.as_deref() != Some(model.as_str()) {
                let _ = tx_event.send(Ok(ResponseEvent::ServerModel(model.clone()))).await;
                last_server_model = Some(model);
            }
        }

        match process_responses_event(event) {
            Ok(Some(event)) => {
                let is_completed = matches!(event, ResponseEvent::Completed { .. });
                let _ = tx_event.send(Ok(event)).await;
                if is_completed {
                    break;
                }
            }
            Ok(None) => {}
            Err(error) => {
                return Err(error.into_api_error());
            }
        }
    }
    Ok(())
}

/// Extract auth token from headers and build WebSocket subprotocols.
/// OpenAI browser WebSocket auth: openai-insecure-api-key.<token>
fn headers_to_protocols(headers: &HeaderMap) -> Vec<String> {
    let mut protocols = Vec::new();
    if let Some(auth) = headers.get(http::header::AUTHORIZATION) {
        if let Ok(val) = auth.to_str() {
            if let Some(token) = val.strip_prefix("Bearer ") {
                protocols.push(format!("openai-insecure-api-key.{token}"));
            }
        }
    }
    // Add the beta header as a subprotocol.
    // WebSocket subprotocols can't contain '=' so replace with '.'
    if let Some(beta) = headers.get("openai-beta") {
        if let Ok(val) = beta.to_str() {
            let sanitized = val.replace('=', ".");
            protocols.push(format!("openai-beta.{sanitized}"));
        }
    }
    protocols
}

fn merge_request_headers(
    provider_headers: &HeaderMap,
    extra_headers: HeaderMap,
    default_headers: HeaderMap,
) -> HeaderMap {
    let mut headers = provider_headers.clone();
    headers.extend(extra_headers);
    for (name, value) in &default_headers {
        if let http::header::Entry::Vacant(entry) = headers.entry(name) {
            entry.insert(value.clone());
        }
    }
    headers
}

pub struct ResponsesWebsocketClient<A: AuthProvider> {
    provider: Provider,
    auth: A,
}

impl<A: AuthProvider> ResponsesWebsocketClient<A> {
    pub fn new(provider: Provider, auth: A) -> Self {
        Self { provider, auth }
    }

    pub async fn connect(
        &self,
        extra_headers: HeaderMap,
        default_headers: HeaderMap,
        _turn_state: Option<Arc<OnceLock<String>>>,
        _telemetry: Option<Arc<dyn crate::telemetry::WebsocketTelemetry>>,
    ) -> Result<ResponsesWebsocketConnection, ApiError> {
        let ws_url = self.provider
            .websocket_url_for_path("responses")
            .map_err(|e| ApiError::Stream(format!("failed to build ws URL: {e}")))?;

        let mut headers = merge_request_headers(
            &self.provider.headers, extra_headers, default_headers,
        );
        add_auth_headers_to_header_map(&self.auth, &mut headers);

        let protocols = headers_to_protocols(&headers);
        info!("connecting to websocket: {ws_url}");

        // Browser WebSocket API cannot send custom auth headers.
        // OpenAI's Responses WebSocket doesn't support subprotocol auth
        // (only the Realtime API does). Return 426 so the upstream client
        // takes its normal HTTP fallback path and disables websockets for
        // the session without retrying a transport that can never work.
        Err(ApiError::Transport(TransportError::Http {
            status: StatusCode::UPGRADE_REQUIRED,
            url: Some(ws_url.to_string()),
            headers: None,
            body: Some("websocket not supported in browser (no header auth)".into()),
        }))
    }
}

// Error parsing — matches upstream protocol for wrapped error events
#[derive(Debug, Deserialize)]
struct WrappedWebsocketError {
    code: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WrappedWebsocketErrorEvent {
    #[serde(rename = "type")]
    kind: String,
    #[serde(alias = "status_code")]
    status: Option<u16>,
    #[serde(default)]
    error: Option<WrappedWebsocketError>,
    #[serde(default)]
    headers: Option<JsonMap<String, Value>>,
}

fn parse_wrapped_websocket_error_event(payload: &str) -> Option<WrappedWebsocketErrorEvent> {
    let event: WrappedWebsocketErrorEvent = serde_json::from_str(payload).ok()?;
    if event.kind != "error" { return None; }
    Some(event)
}

fn map_wrapped_websocket_error_event(
    event: WrappedWebsocketErrorEvent,
    original_payload: String,
) -> Option<ApiError> {
    let WrappedWebsocketErrorEvent { status, error, headers, .. } = event;
    if let Some(error) = error.as_ref() {
        if let Some(code) = error.code.as_deref() {
            if code == WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE {
                return Some(ApiError::Retryable {
                    message: error.message.clone()
                        .unwrap_or_else(|| WEBSOCKET_CONNECTION_LIMIT_REACHED_MESSAGE.to_string()),
                    delay: None,
                });
            }
        }
    }
    let status = StatusCode::from_u16(status?).ok()?;
    if status.is_success() { return None; }
    Some(ApiError::Transport(TransportError::Http {
        status,
        url: None,
        headers: headers.map(json_headers_to_http_headers),
        body: Some(original_payload),
    }))
}

fn json_headers_to_http_headers(headers: JsonMap<String, Value>) -> HeaderMap {
    let mut mapped = HeaderMap::new();
    for (name, value) in headers {
        let Ok(header_name) = HeaderName::from_bytes(name.as_bytes()) else { continue; };
        let Some(header_value) = json_header_value(value) else { continue; };
        mapped.insert(header_name, header_value);
    }
    mapped
}

fn json_header_value(value: Value) -> Option<HeaderValue> {
    let value = match value {
        Value::String(v) => v,
        Value::Number(v) => v.to_string(),
        Value::Bool(v) => v.to_string(),
        _ => return None,
    };
    HeaderValue::from_str(&value).ok()
}
