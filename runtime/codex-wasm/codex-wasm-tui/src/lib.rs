#![allow(unused_variables)]
//! Codex WASM TUI — entry point for the wasip2 component.
//!
//! This crate wraps the upstream Codex TUI (`codex-tui`) as a WASM component
//! that runs in the browser. It replaces the old ratatui-based web-agent-tui.
//!
//! The exported `run()` function initializes backends, constructs a minimal
//! Cli struct, and calls `codex_tui::run_main()` via `tokio::block_on()`.

#[allow(warnings)]
mod bindings;
mod pty_backend;
mod shell_exec_backend;
mod wasi_http_backend;
mod websocket_backend;

use bindings::*;

use clap::Parser;
use codex_arg0::Arg0DispatchPaths;
use codex_core::config_loader::LoaderOverrides;
use codex_tui::Cli;

struct CodexTui;

/// Credential backend that delegates to the WIT credential-store import.
struct WitCredentialBackend;

impl codex_keyring_store::CredentialBackend for WitCredentialBackend {
    fn load(&self, service: &str, account: &str) -> Result<Option<String>, String> {
        bindings::codex::tui::credential_store::load(service, account)
    }
    fn save(&self, service: &str, account: &str, value: &str) -> Result<(), String> {
        bindings::codex::tui::credential_store::save(service, account, value)
    }
    fn delete(&self, service: &str, account: &str) -> Result<bool, String> {
        bindings::codex::tui::credential_store::delete_credential(service, account)
    }
}

/// Yield to the JS event loop via wasi:clocks monotonic-clock subscribe.
/// Called by wasi-tokio's block_on when a future returns Pending.
fn wasm_yield() {
    // Subscribe to a 16ms timer (~60fps) and block on it. Both subscribe_duration
    // and pollable.block are JSPI-suspendable imports, so this yields
    // to the JS event loop. 16ms gives the browser enough time to process
    // fetch responses (SSE chunks), render the terminal, and handle keyboard
    // input between poll iterations.
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
        // Register the PTY backend so unified_exec uses the WIT shell-pty
        // interface for persistent shell sessions instead of stub errors.
        codex_exec_server::set_exec_backend(std::sync::Arc::new(pty_backend::WitPtyBackend));
        codex_keyring_store::set_credential_backend(Box::new(WitCredentialBackend));
        tokio::set_yield_fn(wasm_yield);
        // Register terminal size query → WIT terminal:info/size binding
        // This lets crossterm detect resize without stdin-injected escape sequences.
        crossterm::terminal::set_size_query(|| {
            let dims = bindings::terminal::info::size::get_terminal_size();
            (dims.cols as u16, dims.rows as u16)
        });
        // Register webbrowser shim → WIT browser binding
        webbrowser::set_open_handler(|url| {
            bindings::host::browser::actions::open_url(url).map_err(|e| e.to_string())
        });
        // Register clipboard shims → WIT clipboard binding
        arboard::set_read_handler(|| {
            bindings::host::browser::clipboard::read_text().map_err(|e| e.to_string())
        });
        arboard::set_write_handler(|text| {
            bindings::host::browser::clipboard::write_text(text).map_err(|e| e.to_string())
        });
        // Register audio shims → WIT browser audio binding
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

impl Guest for CodexTui {
    fn push_auth_callback(
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) {
        console_log::console_log!("[codex-wasm-tui] push_auth_callback: {} {}", method, path);
        tiny_http::push_incoming_request(&method, &path, headers, body);
    }

    fn run() -> i32 {
        ensure_initialized();

        console_log::console_log!("[codex-wasm-tui] run() entered, calling block_on");
        tokio::block_on(async {
            console_log::console_log!(
                "[codex-wasm-tui] block_on started, yielding then creating Cli"
            );
            // Yield to JS event loop before heavy startup.
            wasm_yield();

            // Parse CLI args from WASI environment (set by the shell via setArguments)
            let wasi_args = bindings::wasi::cli::environment::get_arguments();
            console_log::console_log!("[codex-wasm-tui] WASI args: {:?}", wasi_args);

            let cli = match Cli::try_parse_from(&wasi_args) {
                Ok(mut cli) => {
                    // Default to bypassing sandbox in WASM (no OS sandboxing available)
                    if cli.sandbox_mode.is_none() && cli.approval_policy.is_none() {
                        cli.dangerously_bypass_approvals_and_sandbox = true;
                    }
                    cli
                }
                Err(e) => {
                    // --help/--version go to stdout (terminal), errors to stderr (console)
                    if e.use_stderr() {
                        console_log::console_error!("{e}");
                    } else {
                        println!("{e}");
                    }
                    return if e.use_stderr() { 1 } else { 0 };
                }
            };

            let arg0_paths = Arg0DispatchPaths {
                codex_self_exe: None,
                codex_linux_sandbox_exe: None,
                main_execve_wrapper_exe: None,
            };

            console_log::console_log!("[codex-wasm-tui] calling run_main...");
            match codex_tui::run_main(cli, arg0_paths, LoaderOverrides::default(), None, None).await
            {
                Ok(exit_info) => match exit_info.exit_reason {
                    codex_tui::ExitReason::UserRequested => 0,
                    codex_tui::ExitReason::Fatal(_) => 1,
                },
                Err(e) => {
                    // Write error to stderr
                    console_log::console_error!("codex-tui error: {e}");
                    1
                }
            }
        })
    }
}

export!(CodexTui with_types_in bindings);
