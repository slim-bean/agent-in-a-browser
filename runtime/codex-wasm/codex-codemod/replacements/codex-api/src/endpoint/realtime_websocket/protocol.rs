//! Stub — realtime websocket protocol types for wasip2.
#![allow(dead_code)]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealtimeEventParser { V1, RealtimeV2 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealtimeSessionMode { TextOnly, AudioOnly, TextAndAudio, Conversational, Transcription }

#[derive(Debug, Clone)]
pub struct RealtimeSessionConfig {
    pub model: Option<String>,
    pub instructions: String,
    pub session_mode: RealtimeSessionMode,
    pub event_parser: RealtimeEventParser,
    pub session_id: Option<String>,
}

impl RealtimeSessionConfig {
    pub fn new(model: Option<String>, instructions: String, session_mode: RealtimeSessionMode, event_parser: RealtimeEventParser) -> Self {
        Self { model, instructions, session_mode, event_parser, session_id: None }
    }
}
