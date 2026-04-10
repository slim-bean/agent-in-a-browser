#![allow(unused_variables)]
//! Codex WASM App-Server — entry point for the wasip2 component.
//!
//! This crate wraps the upstream `codex-app-server-client` in-process runtime
//! as a WASM component. Instead of rendering a TUI, it exposes a JSON protocol
//! bridge via WIT exports so the browser frontend can drive the agent.
//!
//! The exported `start()` function initializes the app-server runtime and spawns
//! an event pump that pushes server events to JS via the `event-sink` import.
//! The `protocol` interface exports let the frontend send requests, notifications,
//! and approval responses back to the app-server.

#[allow(warnings)]
mod bindings;
mod pty_backend;
mod shell_exec_backend;
mod wasi_http_backend;
mod websocket_backend;

use bindings::export;
use bindings::exports::codex::app_server::protocol::Guest as ProtocolGuest;
use bindings::Guest;

use std::sync::Arc;
use std::sync::OnceLock;

use codex_app_server_client::InProcessAppServerClient;
use codex_app_server_client::InProcessAppServerRequestHandle;
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
use codex_feedback::CodexFeedback;
use codex_protocol::protocol::SessionSource;

/// Global app-server request handle (cloneable, used by protocol exports).
static REQUEST_HANDLE: OnceLock<InProcessAppServerRequestHandle> = OnceLock::new();

/// Global client for event consumption and shutdown.
static CLIENT: OnceLock<tokio::sync::Mutex<Option<InProcessAppServerClient>>> = OnceLock::new();

/// Shutdown signal sender — stored by start(), consumed by shutdown().
static SHUTDOWN_TX: OnceLock<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>> =
    OnceLock::new();

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
            let client = match InProcessAppServerClient::start(args).await {
                Ok(client) => client,
                Err(e) => {
                    console_log::console_error!(
                        "[codex-wasm-app-server] app-server start failed: {e}"
                    );
                    return 1;
                }
            };

            console_log::console_log!("[codex-wasm-app-server] app-server started successfully");

            // Store the request handle for use by protocol exports
            let request_handle = client.request_handle();
            let _ = REQUEST_HANDLE.set(request_handle);

            // Spawn event pump: reads events from the in-process client and
            // pushes them to JS via the event-sink WIT import.
            // We move the client into the pump task and store it behind a mutex
            // so shutdown can reclaim it later.
            let client_mutex = tokio::sync::Mutex::new(Some(client));
            let _ = CLIENT.set(client_mutex);

            // Create a oneshot channel for signaling graceful shutdown.
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
            let _ = SHUTDOWN_TX.set(tokio::sync::Mutex::new(Some(shutdown_tx)));

            tokio::spawn(async move {
                let client_lock = CLIENT.get().expect("CLIENT not set");
                // Take the client out of the mutex for the event loop.
                let mut client = {
                    let mut guard = client_lock.lock().await;
                    guard.take().expect("client already taken")
                };

                // Pin the shutdown receiver so we can poll it in select!
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

            0 // success
        })
    }
}

impl ProtocolGuest for CodexAppServer {
    fn send_request(json: String) -> String {
        let handle = match REQUEST_HANDLE.get() {
            Some(h) => h,
            None => {
                return r#"{"error":{"code":-32002,"message":"app-server not started"}}"#
                    .to_string()
            }
        };

        tokio::block_on(async {
            let request: ClientRequest = match serde_json::from_str(&json) {
                Ok(r) => r,
                Err(e) => {
                    return serde_json::to_string(&serde_json::json!({
                        "error": {
                            "code": -32700,
                            "message": format!("invalid request JSON: {e}")
                        }
                    }))
                    .unwrap_or_default();
                }
            };

            match handle.request(request).await {
                Ok(Ok(result)) => {
                    // Successful result — wrap in JSON-RPC result envelope
                    serde_json::to_string(&serde_json::json!({ "result": result }))
                        .unwrap_or_default()
                }
                Ok(Err(error)) => {
                    // App-server returned a JSON-RPC error
                    serde_json::to_string(&serde_json::json!({ "error": error }))
                        .unwrap_or_default()
                }
                Err(e) => serde_json::to_string(&serde_json::json!({
                    "error": {
                        "code": -32603,
                        "message": format!("transport error: {e}")
                    }
                }))
                .unwrap_or_default(),
            }
        })
    }

    fn send_notification(json: String) {
        let handle = match REQUEST_HANDLE.get() {
            Some(h) => h,
            None => {
                console_log::console_error!(
                    "[codex-wasm-app-server] send_notification: not started"
                );
                return;
            }
        };

        tokio::block_on(async {
            let notification: ClientNotification = match serde_json::from_str(&json) {
                Ok(n) => n,
                Err(e) => {
                    console_log::console_error!(
                        "[codex-wasm-app-server] invalid notification JSON: {e}"
                    );
                    return;
                }
            };

            if let Err(e) = handle.notify(notification).await {
                console_log::console_error!("[codex-wasm-app-server] notify error: {e}");
            }
        });
    }

    fn respond_to_server_request(request_id: String, result_json: String) {
        let handle = match REQUEST_HANDLE.get() {
            Some(h) => h,
            None => return,
        };

        tokio::block_on(async {
            let id: RequestId = match serde_json::from_str(&request_id) {
                Ok(id) => id,
                Err(e) => {
                    console_log::console_error!(
                        "[codex-wasm-app-server] invalid request_id JSON: {e}"
                    );
                    return;
                }
            };

            let result: serde_json::Value = match serde_json::from_str(&result_json) {
                Ok(v) => v,
                Err(e) => {
                    console_log::console_error!("[codex-wasm-app-server] invalid result JSON: {e}");
                    return;
                }
            };

            if let Err(e) = handle.resolve_server_request(id, result).await {
                console_log::console_error!(
                    "[codex-wasm-app-server] resolve_server_request error: {e}"
                );
            }
        });
    }

    fn fail_server_request(request_id: String, error_json: String) {
        let handle = match REQUEST_HANDLE.get() {
            Some(h) => h,
            None => return,
        };

        tokio::block_on(async {
            let id: RequestId = match serde_json::from_str(&request_id) {
                Ok(id) => id,
                Err(e) => {
                    console_log::console_error!(
                        "[codex-wasm-app-server] invalid request_id JSON: {e}"
                    );
                    return;
                }
            };

            let error: JSONRPCErrorError = match serde_json::from_str(&error_json) {
                Ok(e) => e,
                Err(e) => {
                    console_log::console_error!("[codex-wasm-app-server] invalid error JSON: {e}");
                    return;
                }
            };

            if let Err(e) = handle.reject_server_request(id, error).await {
                console_log::console_error!(
                    "[codex-wasm-app-server] reject_server_request error: {e}"
                );
            }
        });
    }

    fn shutdown() {
        console_log::console_log!("[codex-wasm-app-server] shutdown requested");
        // Send the shutdown signal to the event pump task, which will call
        // client.shutdown().await before exiting.
        if let Some(tx_mutex) = SHUTDOWN_TX.get() {
            if let Some(tx) = tokio::block_on(async { tx_mutex.lock().await.take() }) {
                let _ = tx.send(());
                console_log::console_log!("[codex-wasm-app-server] shutdown signal sent");
            } else {
                console_log::console_log!(
                    "[codex-wasm-app-server] shutdown signal already consumed"
                );
            }
        }
    }
}

export!(CodexAppServer with_types_in bindings);
