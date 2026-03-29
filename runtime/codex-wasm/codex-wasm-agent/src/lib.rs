#![allow(unused_variables)]
//! Codex WASM Agent — entry point for the wasip2 component.
//!
//! This crate is the cdylib that gets compiled to wasm32-wasip2 and
//! transpiled to JS/ESM via JCO. It implements the same WIT interface
//! as the existing headless-agent, making it a drop-in replacement.
//!
//! Wiring: AgentConfig → codex-core Config+AuthManager+ThreadManager → CodexThread
//! Events: CodexThread::next_event() → EventMsg → AgentEvent (WIT variant)

#[allow(warnings)]
mod bindings;
mod shell_exec_backend;
mod wasi_http_backend;

use bindings::*;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use codex_core::config::{ConfigBuilder, ConfigOverrides};
use codex_core::{CodexThread, ThreadManager};
use codex_core::models_manager::collaboration_mode_presets::CollaborationModesConfig;
use codex_login::{AuthManager, CodexAuth};
use codex_protocol::config_types::SandboxMode;
use codex_protocol::protocol::{
    AskForApproval, Event, EventMsg, Op, SessionSource,
};
use codex_protocol::user_input::UserInput;

/// Global state for active agent sessions.
struct AgentSession {
    thread: Arc<CodexThread>,
    /// Accumulated full message text for stream-complete.
    accumulated_text: String,
    /// Whether we've sent the initial stream-start event.
    stream_started: bool,
}

/// Global registry of active sessions, keyed by handle ID.
static SESSIONS: Mutex<Option<HashMap<u32, AgentSession>>> = Mutex::new(None);
/// Next handle ID to assign.
static NEXT_HANDLE: Mutex<u32> = Mutex::new(1);

fn sessions_map() -> std::sync::MutexGuard<'static, Option<HashMap<u32, AgentSession>>> {
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner())
}

fn with_session<F, R>(handle: u32, f: F) -> Result<R, String>
where
    F: FnOnce(&mut AgentSession) -> Result<R, String>,
{
    let mut guard = sessions_map();
    let map = guard.as_mut().ok_or("sessions not initialized")?;
    let session = map.get_mut(&handle).ok_or_else(|| format!("invalid handle: {handle}"))?;
    f(session)
}

struct CodexAgent;

/// Initialize backends and session registry on first use.
fn ensure_initialized() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        reqwest::backend::set_backend(wasi_http_backend::WasiHttpBackend);
        tokio::process_backend::set_backend(shell_exec_backend::WasiShellBackend);
        // Initialize session registry
        *sessions_map() = Some(HashMap::new());
    });
}

impl Guest for CodexAgent {
    fn create(config: AgentConfig) -> Result<AgentHandle, String> {
        ensure_initialized();

        // Use tokio::block_on to run the async initialization
        tokio::block_on(async {
            create_session(config).await
        })
    }

    fn destroy(handle: AgentHandle) {
        // Shut down the thread and remove from registry
        let thread = {
            let mut guard = sessions_map();
            if let Some(map) = guard.as_mut() {
                map.remove(&handle).map(|s| s.thread)
            } else {
                None
            }
        };

        if let Some(thread) = thread {
            // Best-effort shutdown
            let _ = tokio::block_on(async {
                let _ = thread.submit(Op::Shutdown).await;
                let _ = thread.shutdown_and_wait().await;
            });
        }
    }

    fn send_message(handle: AgentHandle, message: String) -> Result<(), String> {
        ensure_initialized();

        let thread = with_session(handle, |session| {
            // Reset accumulated text for new turn
            session.accumulated_text.clear();
            session.stream_started = false;
            Ok(session.thread.clone())
        })?;

        tokio::block_on(async {
            let op = Op::UserInput {
                items: vec![UserInput::Text {
                    text: message,
                    text_elements: vec![],
                }],
                final_output_json_schema: None,
            };

            thread.submit(op).await.map_err(|e| format!("submit failed: {e}"))?;
            Ok(())
        })
    }

    fn poll(handle: AgentHandle) -> Option<AgentEvent> {
        let thread = {
            let guard = sessions_map();
            let map = guard.as_ref()?;
            let session = map.get(&handle)?;
            session.thread.clone()
        };

        // Try to get the next event (non-blocking via our shim)
        let event_result = tokio::block_on(async {
            thread.next_event().await
        });

        match event_result {
            Ok(event) => translate_event(handle, event),
            Err(_) => Some(AgentEvent::StreamError("event stream ended".into())),
        }
    }

    fn cancel(handle: AgentHandle) {
        let thread = {
            let guard = sessions_map();
            guard.as_ref()
                .and_then(|map| map.get(&handle))
                .map(|s| s.thread.clone())
        };

        if let Some(thread) = thread {
            let _ = tokio::block_on(async {
                thread.submit(Op::Interrupt).await
            });
        }
    }

    fn plan(handle: AgentHandle, message: String) -> Result<(), String> {
        // Plan mode: submit with collaboration mode set to Plan
        // For now, just forward as a regular message
        Self::send_message(handle, message)
    }

    fn execute(handle: AgentHandle) -> Result<(), String> {
        Err("execute not yet implemented — use send_message for auto-approve flow".into())
    }

    fn get_history(handle: AgentHandle) -> Vec<Message> {
        // TODO: Extract conversation history from CodexThread
        Vec::new()
    }

    fn clear_history(handle: AgentHandle) {}

    fn list_providers() -> Vec<ProviderInfo> {
        vec![
            ProviderInfo {
                id: "openai".into(),
                name: "OpenAI".into(),
                default_base_url: Some("https://api.openai.com/v1".into()),
            },
            ProviderInfo {
                id: "anthropic".into(),
                name: "Anthropic (Claude)".into(),
                default_base_url: Some("https://api.anthropic.com".into()),
            },
        ]
    }

    fn list_models(provider_id: String) -> Vec<ModelInfo> {
        match provider_id.as_str() {
            "openai" => vec![
                ModelInfo { id: "o3".into(), name: "o3".into() },
                ModelInfo { id: "o4-mini".into(), name: "o4-mini".into() },
                ModelInfo { id: "gpt-4.1".into(), name: "GPT-4.1".into() },
            ],
            "anthropic" => vec![
                ModelInfo { id: "claude-sonnet-4-6".into(), name: "Claude Sonnet 4.6".into() },
                ModelInfo { id: "claude-opus-4-6".into(), name: "Claude Opus 4.6".into() },
            ],
            _ => vec![],
        }
    }

    fn fetch_models(
        provider_id: String,
        api_key: String,
        base_url: Option<String>,
    ) -> Result<Vec<ModelInfo>, String> {
        ensure_initialized();

        let url = base_url.unwrap_or_else(|| match provider_id.as_str() {
            "openai" => "https://api.openai.com/v1".to_string(),
            _ => "https://api.openai.com/v1".to_string(),
        });
        let models_url = format!("{url}/models");

        let result = reqwest::backend::execute_request(reqwest::backend::RawRequest {
            method: "GET".into(),
            url: models_url,
            headers: vec![
                ("authorization".into(), format!("Bearer {api_key}")),
                ("content-type".into(), "application/json".into()),
            ],
            body: None,
        });

        match result {
            Ok(response) => {
                if response.status >= 400 {
                    return Err(format!(
                        "HTTP {}: {}",
                        response.status,
                        String::from_utf8_lossy(&response.body)
                    ));
                }
                #[derive(serde::Deserialize)]
                struct ModelsResponse {
                    data: Vec<ModelData>,
                }
                #[derive(serde::Deserialize)]
                struct ModelData {
                    id: String,
                }

                match serde_json::from_slice::<ModelsResponse>(&response.body) {
                    Ok(models) => Ok(models
                        .data
                        .into_iter()
                        .map(|m| ModelInfo {
                            id: m.id.clone(),
                            name: m.id,
                        })
                        .collect()),
                    Err(e) => Err(format!("Failed to parse models response: {e}")),
                }
            }
            Err(e) => Err(format!("HTTP request failed: {e}")),
        }
    }
}

/// Create a new codex-core session from the WIT AgentConfig.
async fn create_session(config: AgentConfig) -> Result<AgentHandle, String> {
    let codex_home = PathBuf::from("/tmp/codex-home");
    let cwd = PathBuf::from("/workspace");

    // Create auth from the provided API key
    let auth = CodexAuth::from_api_key(&config.api_key);
    let auth_manager = AuthManager::from_auth_for_testing_with_home(
        auth,
        codex_home.clone(),
    );

    // Build the config with overrides from WIT AgentConfig
    let overrides = ConfigOverrides {
        model: Some(config.model.clone()),
        model_provider: Some(config.provider.clone()),
        cwd: Some(cwd.clone()),
        // Auto-approve everything in WASM — sandboxing is handled by the host
        approval_policy: Some(AskForApproval::Never),
        sandbox_mode: Some(SandboxMode::DangerFullAccess),
        base_instructions: config.preamble.clone(),
        ..Default::default()
    };

    let codex_config = ConfigBuilder::default()
        .codex_home(codex_home)
        .fallback_cwd(Some(cwd))
        .harness_overrides(overrides)
        .build()
        .await
        .map_err(|e| format!("config build failed: {e}"))?;

    // Create ThreadManager
    let thread_manager = ThreadManager::new(
        &codex_config,
        auth_manager,
        SessionSource::Custom("wasm-agent".into()),
        CollaborationModesConfig {
            default_mode_request_user_input: false,
        },
    );

    // Start a thread
    let new_thread = thread_manager
        .start_thread(codex_config)
        .await
        .map_err(|e| format!("start_thread failed: {e}"))?;

    // Allocate a handle
    let handle = {
        let mut h = NEXT_HANDLE.lock().unwrap_or_else(|e| e.into_inner());
        let id = *h;
        *h = id.wrapping_add(1);
        id
    };

    // Store the session
    {
        let mut guard = sessions_map();
        let map = guard.as_mut().ok_or("sessions not initialized")?;
        map.insert(handle, AgentSession {
            thread: new_thread.thread,
            accumulated_text: String::new(),
            stream_started: false,
        });
    }

    Ok(handle)
}

/// Translate a codex-core Event into a WIT AgentEvent.
fn translate_event(handle: u32, event: Event) -> Option<AgentEvent> {
    let Event { id, msg } = event;

    match msg {
        // === Session lifecycle ===
        EventMsg::SessionConfigured(_) => {
            Some(AgentEvent::Ready)
        }

        // === Turn lifecycle ===
        EventMsg::TurnStarted(_) => {
            // Mark stream as started, emit StreamStart
            let mut guard = sessions_map();
            if let Some(map) = guard.as_mut() {
                if let Some(session) = map.get_mut(&handle) {
                    session.stream_started = true;
                    session.accumulated_text.clear();
                }
            }
            Some(AgentEvent::StreamStart)
        }

        EventMsg::TurnComplete(tc) => {
            // Emit the final accumulated text
            let text = {
                let mut guard = sessions_map();
                guard.as_mut()
                    .and_then(|map| map.get_mut(&handle))
                    .map(|session| {
                        let t = session.accumulated_text.clone();
                        session.stream_started = false;
                        t
                    })
                    .unwrap_or_default()
            };
            let final_text = tc.last_agent_message.unwrap_or(text);
            Some(AgentEvent::StreamComplete(final_text))
        }

        EventMsg::TurnAborted(_) => {
            Some(AgentEvent::StreamError("turn aborted".into()))
        }

        // === Agent message streaming ===
        EventMsg::AgentMessageDelta(delta) => {
            // Accumulate text and emit chunk
            let mut guard = sessions_map();
            if let Some(map) = guard.as_mut() {
                if let Some(session) = map.get_mut(&handle) {
                    session.accumulated_text.push_str(&delta.delta);
                }
            }
            Some(AgentEvent::StreamChunk(delta.delta))
        }

        EventMsg::AgentMessage(msg) => {
            // Full message (non-streaming path)
            let mut guard = sessions_map();
            if let Some(map) = guard.as_mut() {
                if let Some(session) = map.get_mut(&handle) {
                    session.accumulated_text.clone_from(&msg.message);
                }
            }
            Some(AgentEvent::StreamChunk(msg.message))
        }

        // === Tool execution ===
        EventMsg::ExecCommandBegin(exec) => {
            let cmd_str = exec.command.join(" ");
            // Serialize as JSON for richer tool-call info
            let tool_call_json = serde_json::json!({
                "type": "exec",
                "call_id": exec.call_id,
                "command": cmd_str,
                "cwd": exec.cwd.display().to_string(),
            });
            Some(AgentEvent::ToolCall(tool_call_json.to_string()))
        }

        EventMsg::ExecCommandEnd(exec) => {
            let output = if exec.stdout.is_empty() {
                exec.stderr.clone()
            } else {
                exec.stdout.clone()
            };
            Some(AgentEvent::ToolResult(ToolResultData {
                name: "exec".into(),
                output,
                is_error: exec.exit_code != 0,
            }))
        }

        // === Patch/apply operations ===
        EventMsg::PatchApplyBegin(patch) => {
            let tool_call_json = serde_json::json!({
                "type": "patch",
                "call_id": patch.call_id,
            });
            Some(AgentEvent::ToolCall(tool_call_json.to_string()))
        }

        EventMsg::PatchApplyEnd(patch) => {
            Some(AgentEvent::ToolResult(ToolResultData {
                name: "patch".into(),
                output: serde_json::to_string(&patch).unwrap_or_default(),
                is_error: false,
            }))
        }

        // === MCP tool calls ===
        EventMsg::McpToolCallBegin(mcp) => {
            let tool_call_json = serde_json::json!({
                "type": "mcp",
                "server": mcp.invocation.server,
                "tool": mcp.invocation.tool,
            });
            Some(AgentEvent::ToolCall(tool_call_json.to_string()))
        }

        EventMsg::McpToolCallEnd(mcp) => {
            let (output, is_error) = match &mcp.result {
                Ok(result) => (serde_json::to_string(result).unwrap_or_default(), false),
                Err(e) => (e.clone(), true),
            };
            Some(AgentEvent::ToolResult(ToolResultData {
                name: format!("mcp:{}", mcp.invocation.tool),
                output,
                is_error,
            }))
        }

        // === Errors ===
        EventMsg::Error(err) => {
            Some(AgentEvent::StreamError(err.message))
        }

        EventMsg::StreamError(err) => {
            Some(AgentEvent::StreamError(err.message))
        }

        EventMsg::Warning(warn) => {
            // Warnings are non-fatal; surface as stream chunks
            Some(AgentEvent::StreamChunk(format!("[warning] {}", warn.message)))
        }

        // === Shutdown ===
        EventMsg::ShutdownComplete => {
            None
        }

        // === All other events — ignore for now ===
        _ => None,
    }
}

export!(CodexAgent with_types_in bindings);
