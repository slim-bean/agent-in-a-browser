//! Stub — realtime websocket methods for wasip2.
#![allow(dead_code, unused_variables)]
use crate::error::ApiError;
use crate::provider::Provider;
use super::protocol::{RealtimeSessionConfig, RealtimeEventParser};
use codex_protocol::protocol::RealtimeEvent;

pub struct RealtimeWebsocketConnection {
    _private: (),
}
impl RealtimeWebsocketConnection {
    pub fn writer(&self) -> RealtimeWebsocketWriter { RealtimeWebsocketWriter }
    pub fn events(&self) -> RealtimeWebsocketEvents { RealtimeWebsocketEvents }
}

#[derive(Clone)]
pub struct RealtimeWebsocketWriter;
impl RealtimeWebsocketWriter {
    pub async fn send(&self, _msg: &str) -> Result<(), ApiError> { Err(ApiError::Stream("WASM".into())) }
    pub async fn close(&self) -> Result<(), ApiError> { Ok(()) }
    pub async fn send_conversation_item_create(&self, _text: impl Into<String>) -> Result<(), ApiError> { Err(ApiError::Stream("WASM".into())) }
    pub async fn send_response_create(&self) -> Result<(), ApiError> { Err(ApiError::Stream("WASM".into())) }
    pub async fn send_audio_frame<T>(&self, _frame: T) -> Result<(), ApiError> { Err(ApiError::Stream("WASM".into())) }
    pub async fn send_conversation_handoff_append(&self, _handoff_id: impl Into<String>, _text: impl Into<String>) -> Result<(), ApiError> { Err(ApiError::Stream("WASM".into())) }
    pub async fn send_payload(&self, _payload: impl serde::Serialize) -> Result<(), ApiError> { Err(ApiError::Stream("WASM".into())) }
}

pub struct RealtimeWebsocketEvents;
impl RealtimeWebsocketEvents {
    pub async fn next(&mut self) -> Option<Result<RealtimeEvent, ApiError>> { None }
    pub async fn next_event(&mut self) -> Result<Option<RealtimeEvent>, ApiError> { Ok(None) }
}

pub struct RealtimeWebsocketClient {
    _provider: Provider,
}
impl RealtimeWebsocketClient {
    pub fn new(provider: Provider) -> Self { Self { _provider: provider } }
    pub async fn connect(
        &self, _config: RealtimeSessionConfig,
        _extra_headers: http::HeaderMap,
        _default_headers: http::HeaderMap,
    ) -> Result<RealtimeWebsocketConnection, ApiError> {
        Err(ApiError::Stream("realtime websocket not available in WASM".into()))
    }
}
