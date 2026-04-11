#![allow(unused_variables)]
//! Codex WASM App-Server — entry point for the wasip2 component.
//!
//! This crate wraps the upstream `codex-app-server-client` in-process runtime
//! as a WASM component. Instead of rendering a TUI, it exposes a JSON protocol
//! bridge via WIT exports so the browser frontend can drive the agent.
//!
//! The exported `start()` function initializes the app-server runtime, spawns
//! an event pump that pushes server events to JS via the `event-sink` import,
//! and then enters an inbox loop that processes messages pushed by JS via the
//! `protocol-inbox` export. The inbox loop never returns (until shutdown).

#[allow(warnings)]
mod bindings;
mod pty_backend;
mod shell_exec_backend;
mod wasi_http_backend;
mod websocket_backend;

mod inbox {
    use std::sync::mpsc;
    use std::sync::Mutex;
    use std::sync::OnceLock;

    type InboxChannel = (Mutex<mpsc::Sender<String>>, Mutex<mpsc::Receiver<String>>);

    static INBOX: OnceLock<InboxChannel> = OnceLock::new();

    fn channel() -> &'static InboxChannel {
        INBOX.get_or_init(|| {
            let (tx, rx) = mpsc::channel();
            (Mutex::new(tx), Mutex::new(rx))
        })
    }

    /// Push a message into the inbox (called by WIT export push-message).
    pub fn push(json: String) {
        let (tx_lock, _) = channel();
        if let Ok(tx) = tx_lock.lock() {
            let _ = tx.send(json);
        }
    }

    /// Non-blocking attempt to receive from inbox.
    pub fn try_recv() -> Option<String> {
        let (_, rx_lock) = channel();
        if let Ok(rx) = rx_lock.lock() {
            rx.try_recv().ok()
        } else {
            None
        }
    }
}

use bindings::export;
use bindings::Guest;

use std::sync::Arc;

use codex_app_server_client::InProcessAppServerClient;
use codex_app_server_client::InProcessClientStartArgs;
use codex_app_server_client::InProcessServerEvent;
use codex_app_server_client::DEFAULT_IN_PROCESS_CHANNEL_CAPACITY;
use codex_app_server_protocol::ClientNotification;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::RequestId;
use codex_arg0::Arg0DispatchPaths;
use codex_core::config::Config;
use codex_core::config::ConfigBuilder;
use codex_core::config_loader::{CloudRequirementsLoader, LoaderOverrides};
use codex_exec_server::EnvironmentManager;
use codex_feedback::CodexFeedback;
use codex_protocol::protocol::SessionSource;

#[derive(serde::Deserialize)]
#[serde(tag = "type")]
enum InboxMessage {
    #[serde(rename = "request")]
    Request { id: String, json: String },
    #[serde(rename = "notification")]
    Notification { json: String },
    #[serde(rename = "resolve")]
    ResolveServerRequest {
        request_id: String,
        result_json: String,
    },
    #[serde(rename = "reject")]
    RejectServerRequest {
        request_id: String,
        error_json: String,
    },
    #[serde(rename = "shutdown")]
    Shutdown,
}

struct CodexAppServer;

/// Credential backend that delegates to the WIT credential-store import.
struct WitCredentialBackend;

impl codex_keyring_store::CredentialBackend for WitCredentialBackend {
    fn load(&self, service: &str, account: &str) -> Result<Option<String>, String> {
        bindings::codex::app_server::credential_store::load(service, account)
    }
    fn save(&self, service: &str, account: &str, value: &str) -> Result<(), String> {
        bindings::codex::app_server::credential_store::save(service, account, value)
    }
    fn delete(&self, service: &str, account: &str) -> Result<bool, String> {
        bindings::codex::app_server::credential_store::delete_credential(service, account)
    }
}

/// Yield to the JS event loop via wasi:clocks monotonic-clock subscribe.
/// Called by wasi-tokio's block_on when a future returns Pending.
fn wasm_yield() {
    let duration = bindings::wasi::clocks::monotonic_clock::subscribe_duration(1_000_000); // 1ms in ns
    duration.block();
    // Poll for captured audio data and feed it to the stored callback.
    cpal::poll_audio();
}

/// Initialize backends on first use.
fn ensure_initialized() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        reqwest::backend::set_backend(wasi_http_backend::WasiHttpBackend);
        tokio::process_backend::set_backend(shell_exec_backend::WasiShellBackend);
        tokio::websocket_backend::set_backend(websocket_backend::WasiWebSocketBackend);
        codex_exec_server::set_exec_backend(Arc::new(pty_backend::WitPtyBackend));
        codex_keyring_store::set_credential_backend(Box::new(WitCredentialBackend));
        tokio::set_yield_fn(wasm_yield);
        // Register webbrowser shim -> WIT browser binding
        webbrowser::set_open_handler(|url| {
            bindings::host::browser::actions::open_url(url).map_err(|e| e.to_string())
        });
        // Register clipboard shims -> WIT clipboard binding
        arboard::set_read_handler(|| {
            bindings::host::browser::clipboard::read_text().map_err(|e| e.to_string())
        });
        arboard::set_write_handler(|text| {
            bindings::host::browser::clipboard::write_text(text).map_err(|e| e.to_string())
        });
        // Register audio shims -> WIT browser audio binding
        cpal::set_list_devices_handler(|is_input| {
            if is_input {
                bindings::host::browser::audio::list_input_devices()
            } else {
                bindings::host::browser::audio::list_output_devices()
            }
        });
        cpal::set_default_config_handler(|is_input| {
            if is_input {
                bindings::host::browser::audio::default_input_config()
            } else {
                bindings::host::browser::audio::default_output_config()
            }
        });
        cpal::set_start_capture_handler(|device_name, sample_rate, channels| {
            bindings::host::browser::audio::start_capture(device_name, sample_rate, channels)
        });
        cpal::set_read_capture_data_handler(|capture_id| {
            bindings::host::browser::audio::read_capture_data(capture_id)
        });
        cpal::set_get_capture_peak_handler(|capture_id| {
            bindings::host::browser::audio::get_capture_peak(capture_id)
        });
        cpal::set_stop_capture_handler(|capture_id| {
            bindings::host::browser::audio::stop_capture(capture_id)
        });
        cpal::set_start_playback_handler(|device_name, sample_rate, channels| {
            bindings::host::browser::audio::start_playback(device_name, sample_rate, channels)
        });
        cpal::set_enqueue_playback_handler(|player_id, data| {
            bindings::host::browser::audio::enqueue_playback(player_id, data)
        });
        cpal::set_clear_playback_handler(|player_id| {
            bindings::host::browser::audio::clear_playback(player_id)
        });
        cpal::set_stop_playback_handler(|player_id| {
            bindings::host::browser::audio::stop_playback(player_id)
        });
    });
}

/// Serialize an InProcessServerEvent to JSON for the event-sink bridge.
fn serialize_event(event: &InProcessServerEvent) -> String {
    match event {
        InProcessServerEvent::ServerRequest(req) => {
            // Wrap with type tag so JS can distinguish request vs notification
            match serde_json::to_string(req) {
                Ok(json) => format!(r#"{{"type":"request","data":{json}}}"#),
                Err(e) => format!(r#"{{"type":"error","message":"serialize error: {e}"}}"#),
            }
        }
        InProcessServerEvent::ServerNotification(notif) => match serde_json::to_string(notif) {
            Ok(json) => format!(r#"{{"type":"notification","data":{json}}}"#),
            Err(e) => format!(r#"{{"type":"error","message":"serialize error: {e}"}}"#),
        },
        InProcessServerEvent::Lagged { skipped } => {
            format!(r#"{{"type":"lagged","skipped":{skipped}}}"#)
        }
    }
}

impl Guest for CodexAppServer {
    fn push_auth_callback(
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) {
        console_log::console_log!(
            "[codex-wasm-app-server] push_auth_callback: {} {}",
            method,
            path
        );
        tiny_http::push_incoming_request(&method, &path, headers, body);
    }

    fn start() -> i32 {
        ensure_initialized();

        console_log::console_log!("[codex-wasm-app-server] start() entered");
        tokio::block_on(async {
            wasm_yield();

            // Load config from OPFS-backed filesystem
            console_log::console_log!("[codex-wasm-app-server] loading config...");
            let (config, config_warnings) = match ConfigBuilder::default()
                .loader_overrides(LoaderOverrides::default())
                .build()
                .await
            {
                Ok(config) => {
                    let warnings = config
                        .startup_warnings
                        .iter()
                        .map(
                            |w: &String| codex_app_server_protocol::ConfigWarningNotification {
                                summary: w.clone(),
                                details: None,
                                path: None,
                                range: None,
                            },
                        )
                        .collect();
                    (config, warnings)
                }
                Err(e) => {
                    console_log::console_error!(
                        "[codex-wasm-app-server] config error, using defaults: {e}"
                    );
                    match Config::load_default_with_cli_overrides(vec![]) {
                        Ok(config) => (config, vec![]),
                        Err(e2) => {
                            console_log::console_error!(
                                "[codex-wasm-app-server] fatal config error: {e2}"
                            );
                            return 1;
                        }
                    }
                }
            };

            let feedback = CodexFeedback::new();

            let args = InProcessClientStartArgs {
                arg0_paths: Arg0DispatchPaths {
                    codex_self_exe: None,
                    codex_linux_sandbox_exe: None,
                    main_execve_wrapper_exe: None,
                },
                config: Arc::new(config),
                cli_overrides: vec![],
                loader_overrides: LoaderOverrides::default(),
                cloud_requirements: CloudRequirementsLoader::default(),
                feedback,
                environment_manager: Arc::new(EnvironmentManager::from_env()),
                config_warnings,
                session_source: SessionSource::Cli,
                enable_codex_api_key_env: true,
                client_name: "edge-agent".to_string(),
                client_version: env!("CARGO_PKG_VERSION").to_string(),
                experimental_api: true,
                opt_out_notification_methods: vec![],
                channel_capacity: DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
            };

            console_log::console_log!("[codex-wasm-app-server] starting in-process app-server...");
            let mut client = match InProcessAppServerClient::start(args).await {
                Ok(client) => client,
                Err(e) => {
                    console_log::console_error!(
                        "[codex-wasm-app-server] app-server start failed: {e}"
                    );
                    return 1;
                }
            };

            console_log::console_log!("[codex-wasm-app-server] app-server started successfully");

            let request_handle = client.request_handle();

            // Create shutdown channel for the event pump
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

            // Spawn event pump: reads events from the in-process client and
            // pushes them to JS via the event-sink WIT import.
            tokio::spawn(async move {
                tokio::pin!(shutdown_rx);
                loop {
                    tokio::select! {
                        event = client.next_event() => {
                            match event {
                                Some(event) => {
                                    let json = serialize_event(&event);
                                    bindings::codex::app_server::event_sink::emit_event(&json);
                                }
                                None => {
                                    console_log::console_log!("[codex-wasm-app-server] event stream ended");
                                    break;
                                }
                            }
                        }
                        _ = &mut shutdown_rx => {
                            console_log::console_log!("[codex-wasm-app-server] shutdown signal received, shutting down client...");
                            let _ = client.shutdown().await;
                            console_log::console_log!("[codex-wasm-app-server] client shutdown complete");
                            break;
                        }
                    }
                }
            });

            // Main inbox loop — THIS NEVER RETURNS (until shutdown).
            // recv() blocks via thread::sleep which JSPI-suspends, allowing:
            // - JS to push messages via push-message export
            // - tokio background tasks (device code polling, timers) to progress
            // Signal JS that initialization is complete and inbox is ready.
            // This event is emitted before the blocking recv() loop so the
            // JS Worker can tell the main thread that boot succeeded.
            bindings::codex::app_server::event_sink::emit_event(r#"{"type":"started"}"#);

            console_log::console_log!("[codex-wasm-app-server] entering inbox loop...");
            loop {
                // Use try_recv + async sleep so the main future yields Pending
                // back to block_on, allowing spawned tasks (device code polling,
                // timers, etc.) to make progress between inbox checks.
                let raw = loop {
                    if let Some(msg) = inbox::try_recv() {
                        break msg;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                };

                let msg: InboxMessage = match serde_json::from_str(&raw) {
                    Ok(m) => m,
                    Err(e) => {
                        console_log::console_error!(
                            "[codex-wasm-app-server] invalid inbox message: {e}"
                        );
                        continue;
                    }
                };

                match msg {
                    InboxMessage::Request { id, json } => {
                        let request: ClientRequest = match serde_json::from_str(&json) {
                            Ok(r) => r,
                            Err(e) => {
                                let error_resp = serde_json::json!({
                                    "type": "response",
                                    "id": id,
                                    "error": {
                                        "code": -32700,
                                        "message": format!("invalid request JSON: {e}")
                                    }
                                });
                                bindings::codex::app_server::event_sink::emit_event(
                                    &error_resp.to_string(),
                                );
                                continue;
                            }
                        };

                        match request_handle.request(request).await {
                            Ok(response) => {
                                // The response type serializes as {"Ok": ...} or {"Err": ...}
                                // because it's a Rust Result. Unwrap the Ok variant for JS.
                                let value = serde_json::to_value(&response).ok();
                                let id_json = serde_json::to_string(&id)
                                    .unwrap_or_else(|_| "null".to_string());
                                let envelope = match value {
                                    Some(serde_json::Value::Object(ref map))
                                        if map.contains_key("Ok") =>
                                    {
                                        let result_json =
                                            serde_json::to_string(&map["Ok"]).unwrap_or_else(
                                                |e| format!(r#"{{"error":"serialize: {e}"}}"#),
                                            );
                                        format!(
                                            r#"{{"type":"response","id":{id_json},"result":{result_json}}}"#,
                                        )
                                    }
                                    Some(serde_json::Value::Object(ref map))
                                        if map.contains_key("Err") =>
                                    {
                                        let err_msg =
                                            serde_json::to_string(&map["Err"]).unwrap_or_else(
                                                |_| r#""unknown error""#.to_string(),
                                            );
                                        format!(
                                            r#"{{"type":"response","id":{id_json},"error":{{"code":-32603,"message":{err_msg}}}}}"#,
                                        )
                                    }
                                    Some(v) => {
                                        let result_json =
                                            serde_json::to_string(&v).unwrap_or_else(|e| {
                                                format!(r#"{{"error":"serialize: {e}"}}"#)
                                            });
                                        format!(
                                            r#"{{"type":"response","id":{id_json},"result":{result_json}}}"#,
                                        )
                                    }
                                    None => {
                                        format!(
                                            r#"{{"type":"response","id":{id_json},"error":{{"code":-32603,"message":"serialize failed"}}}}"#,
                                        )
                                    }
                                };
                                bindings::codex::app_server::event_sink::emit_event(&envelope);
                            }
                            Err(e) => {
                                let error_resp = serde_json::json!({
                                    "type": "response",
                                    "id": id,
                                    "error": {
                                        "code": -32603,
                                        "message": format!("{e}")
                                    }
                                });
                                bindings::codex::app_server::event_sink::emit_event(
                                    &error_resp.to_string(),
                                );
                            }
                        }
                    }
                    InboxMessage::Notification { json } => {
                        let notification: ClientNotification = match serde_json::from_str(&json) {
                            Ok(n) => n,
                            Err(e) => {
                                console_log::console_error!(
                                    "[codex-wasm-app-server] invalid notification JSON: {e}"
                                );
                                continue;
                            }
                        };
                        if let Err(e) = request_handle.notify(notification).await {
                            console_log::console_error!(
                                "[codex-wasm-app-server] notify error: {e}"
                            );
                        }
                    }
                    InboxMessage::ResolveServerRequest {
                        request_id,
                        result_json,
                    } => {
                        let id: RequestId = match serde_json::from_str(&request_id) {
                            Ok(id) => id,
                            Err(e) => {
                                console_log::console_error!(
                                    "[codex-wasm-app-server] invalid request_id: {e}"
                                );
                                continue;
                            }
                        };
                        let result: serde_json::Value = match serde_json::from_str(&result_json) {
                            Ok(v) => v,
                            Err(e) => {
                                console_log::console_error!(
                                    "[codex-wasm-app-server] invalid result JSON: {e}"
                                );
                                continue;
                            }
                        };
                        if let Err(e) = request_handle.resolve_server_request(id, result).await {
                            console_log::console_error!(
                                "[codex-wasm-app-server] resolve error: {e}"
                            );
                        }
                    }
                    InboxMessage::RejectServerRequest {
                        request_id,
                        error_json,
                    } => {
                        let id: RequestId = match serde_json::from_str(&request_id) {
                            Ok(id) => id,
                            Err(e) => {
                                console_log::console_error!(
                                    "[codex-wasm-app-server] invalid request_id: {e}"
                                );
                                continue;
                            }
                        };
                        let error: JSONRPCErrorError = match serde_json::from_str(&error_json) {
                            Ok(v) => v,
                            Err(e) => {
                                console_log::console_error!(
                                    "[codex-wasm-app-server] invalid error JSON: {e}"
                                );
                                continue;
                            }
                        };
                        if let Err(e) = request_handle.reject_server_request(id, error).await {
                            console_log::console_error!(
                                "[codex-wasm-app-server] reject error: {e}"
                            );
                        }
                    }
                    InboxMessage::Shutdown => {
                        console_log::console_log!(
                            "[codex-wasm-app-server] shutdown requested via inbox"
                        );
                        let _ = shutdown_tx.send(());
                        break;
                    }
                }
            }

            0 // exit code
        })
    }
}

impl bindings::exports::codex::app_server::protocol_inbox::Guest for CodexAppServer {
    fn push_message(json: String) {
        inbox::push(json);
    }
}

export!(CodexAppServer with_types_in bindings);
