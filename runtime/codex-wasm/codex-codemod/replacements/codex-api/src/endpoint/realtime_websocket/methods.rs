//! Realtime WebSocket methods for wasip2 — uses WIT websocket backend
//! instead of tokio-tungstenite. Protocol parsing and message building
//! are provided by the upstream protocol/methods modules (pure Rust).
#![allow(dead_code, unused_variables)]

use super::methods_common::{
    conversation_handoff_append_message, conversation_item_create_message,
    normalized_session_mode, session_update_session, websocket_intent,
};
use super::protocol::{
    parse_realtime_event, RealtimeAudioFrame, RealtimeEvent, RealtimeEventParser,
    RealtimeOutboundMessage, RealtimeSessionConfig, RealtimeSessionMode,
};
use crate::error::ApiError;
use crate::provider::Provider;
use http::HeaderMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use url::Url;

// ---------------------------------------------------------------------------
// RealtimeWebsocketConnection
// ---------------------------------------------------------------------------

pub struct RealtimeWebsocketConnection {
    writer: RealtimeWebsocketWriter,
    event_parser: RealtimeEventParser,
    handle: u32,
}

impl RealtimeWebsocketConnection {
    fn new(handle: u32, event_parser: RealtimeEventParser) -> Self {
        Self {
            writer: RealtimeWebsocketWriter {
                handle,
                event_parser,
                is_closed: Arc::new(AtomicBool::new(false)),
            },
            event_parser,
            handle,
        }
    }

    pub fn writer(&self) -> RealtimeWebsocketWriter {
        self.writer.clone()
    }

    pub fn events(&self) -> RealtimeWebsocketEvents {
        RealtimeWebsocketEvents {
            handle: self.handle,
            event_parser: self.event_parser,
        }
    }
}

// ---------------------------------------------------------------------------
// RealtimeWebsocketWriter
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct RealtimeWebsocketWriter {
    handle: u32,
    event_parser: RealtimeEventParser,
    is_closed: Arc<AtomicBool>,
}

impl RealtimeWebsocketWriter {
    async fn send_json(&self, msg: &RealtimeOutboundMessage) -> Result<(), ApiError> {
        if self.is_closed.load(Ordering::SeqCst) {
            return Err(ApiError::Stream("websocket is closed".into()));
        }
        let json = serde_json::to_string(msg)
            .map_err(|e| ApiError::Stream(format!("failed to serialize message: {e}")))?;
        tokio::websocket_backend::send(self.handle, &json)
            .map_err(|e| ApiError::Stream(format!("websocket send failed: {e}")))?;
        Ok(())
    }

    pub async fn send(&self, msg: &str) -> Result<(), ApiError> {
        if self.is_closed.load(Ordering::SeqCst) {
            return Err(ApiError::Stream("websocket is closed".into()));
        }
        tokio::websocket_backend::send(self.handle, msg)
            .map_err(|e| ApiError::Stream(format!("websocket send failed: {e}")))?;
        Ok(())
    }

    pub async fn send_audio_frame(&self, frame: RealtimeAudioFrame) -> Result<(), ApiError> {
        self.send_json(&RealtimeOutboundMessage::InputAudioBufferAppend {
            audio: frame.data,
        })
        .await
    }

    pub async fn send_conversation_item_create(
        &self,
        text: impl Into<String>,
    ) -> Result<(), ApiError> {
        let msg = conversation_item_create_message(self.event_parser, text.into());
        self.send_json(&msg).await
    }

    pub async fn send_response_create(&self) -> Result<(), ApiError> {
        self.send_json(&RealtimeOutboundMessage::ResponseCreate)
            .await
    }

    pub async fn send_session_update(
        &self,
        instructions: String,
        session_mode: RealtimeSessionMode,
    ) -> Result<(), ApiError> {
        let session_mode = normalized_session_mode(self.event_parser, session_mode);
        let session = session_update_session(self.event_parser, instructions, session_mode);
        self.send_json(&RealtimeOutboundMessage::SessionUpdate { session })
            .await
    }

    pub async fn send_conversation_handoff_append(
        &self,
        handoff_id: impl Into<String>,
        text: impl Into<String>,
    ) -> Result<(), ApiError> {
        let msg = conversation_handoff_append_message(
            self.event_parser,
            handoff_id.into(),
            text.into(),
        );
        self.send_json(&msg).await
    }

    pub async fn send_payload(&self, payload: impl serde::Serialize) -> Result<(), ApiError> {
        let json = serde_json::to_string(&payload)
            .map_err(|e| ApiError::Stream(format!("failed to serialize payload: {e}")))?;
        self.send(&json).await
    }

    pub async fn close(&self) -> Result<(), ApiError> {
        if self.is_closed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        tokio::websocket_backend::close(self.handle);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// RealtimeWebsocketEvents
// ---------------------------------------------------------------------------

pub struct RealtimeWebsocketEvents {
    handle: u32,
    event_parser: RealtimeEventParser,
}

impl RealtimeWebsocketEvents {
    pub async fn next(&mut self) -> Option<Result<RealtimeEvent, ApiError>> {
        match self.next_event().await {
            Ok(Some(event)) => Some(Ok(event)),
            Ok(None) => None,
            Err(err) => Some(Err(err)),
        }
    }

    pub async fn next_event(&mut self) -> Result<Option<RealtimeEvent>, ApiError> {
        match tokio::websocket_backend::recv(self.handle) {
            Ok(Some(text)) => {
                let event = parse_realtime_event(&text, self.event_parser);
                Ok(event)
            }
            Ok(None) => Ok(None), // Connection closed
            Err(e) => Err(ApiError::Stream(format!("websocket recv failed: {e}"))),
        }
    }
}

// ---------------------------------------------------------------------------
// RealtimeWebsocketClient
// ---------------------------------------------------------------------------

pub struct RealtimeWebsocketClient {
    provider: Provider,
}

impl RealtimeWebsocketClient {
    pub fn new(provider: Provider) -> Self {
        Self { provider }
    }

    pub async fn connect(
        &self,
        config: RealtimeSessionConfig,
        extra_headers: HeaderMap,
        default_headers: HeaderMap,
    ) -> Result<RealtimeWebsocketConnection, ApiError> {
        let ws_url = websocket_url_from_api_url(
            self.provider.base_url.as_str(),
            self.provider.query_params.as_ref(),
            config.model.as_deref(),
            config.event_parser,
            config.session_mode,
        )?;

        // Extract bearer token from provider headers or extra_headers for subprotocol auth
        let bearer_token = self
            .provider
            .headers
            .get("authorization")
            .or_else(|| extra_headers.get("authorization"))
            .or_else(|| default_headers.get("authorization"))
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));

        // Build subprotocols — browser WebSocket API can't send custom headers,
        // so we use the OpenAI subprotocol auth mechanism
        let mut protocols = vec!["realtime".to_string()];
        if let Some(token) = bearer_token {
            protocols.push(format!("openai-insecure-api-key.{token}"));
        }

        // If session_id is specified, include it as a subprotocol
        if let Some(session_id) = &config.session_id {
            protocols.push(format!("openai-session-id.{session_id}"));
        }

        let url_str = ws_url.as_str();
        eprintln!("[realtime-ws] connecting to {url_str}");

        let handle = tokio::websocket_backend::connect(url_str, &protocols)
            .map_err(|e| ApiError::Stream(format!("failed to connect realtime websocket: {e}")))?;

        eprintln!("[realtime-ws] connected, sending session.update");

        let connection = RealtimeWebsocketConnection::new(handle, config.event_parser);

        // Send initial session.update
        connection
            .writer
            .send_session_update(config.instructions, config.session_mode)
            .await?;

        Ok(connection)
    }
}

// ---------------------------------------------------------------------------
// URL building (reused from upstream, no tungstenite dependency)
// ---------------------------------------------------------------------------

fn websocket_url_from_api_url(
    api_url: &str,
    query_params: Option<&HashMap<String, String>>,
    model: Option<&str>,
    event_parser: RealtimeEventParser,
    _session_mode: RealtimeSessionMode,
) -> Result<Url, ApiError> {
    let mut url = Url::parse(api_url)
        .map_err(|err| ApiError::Stream(format!("failed to parse realtime api_url: {err}")))?;

    normalize_realtime_path(&mut url);

    match url.scheme() {
        "ws" | "wss" => {}
        "http" | "https" => {
            let scheme = if url.scheme() == "http" { "ws" } else { "wss" };
            let _ = url.set_scheme(scheme);
        }
        scheme => {
            return Err(ApiError::Stream(format!(
                "unsupported realtime api_url scheme: {scheme}"
            )));
        }
    }

    let intent = websocket_intent(event_parser);
    let has_extra_query_params = query_params.is_some_and(|query_params| {
        query_params
            .iter()
            .any(|(key, _)| key != "intent" && !(key == "model" && model.is_some()))
    });
    if intent.is_some() || model.is_some() || has_extra_query_params {
        let mut query = url.query_pairs_mut();
        if let Some(intent) = intent {
            query.append_pair("intent", intent);
        }
        if let Some(model) = model {
            query.append_pair("model", model);
        }
        if let Some(query_params) = query_params {
            for (key, value) in query_params {
                if key == "intent" || (key == "model" && model.is_some()) {
                    continue;
                }
                query.append_pair(key, value);
            }
        }
    }

    Ok(url)
}

fn normalize_realtime_path(url: &mut Url) {
    let path = url.path().to_string();
    if path.is_empty() || path == "/" {
        url.set_path("/v1/realtime");
        return;
    }
    if path.ends_with("/realtime") {
        return;
    }
    if path.ends_with("/realtime/") {
        url.set_path(path.trim_end_matches('/'));
        return;
    }
    if path.ends_with("/v1") {
        url.set_path(&format!("{path}/realtime"));
        return;
    }
    if path.ends_with("/v1/") {
        url.set_path(&format!("{path}realtime"));
    }
}
