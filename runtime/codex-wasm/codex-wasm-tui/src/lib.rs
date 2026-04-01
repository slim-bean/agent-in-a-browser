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
mod shell_exec_backend;
mod wasi_http_backend;
mod websocket_backend;

use bindings::*;

use clap::Parser;
use codex_arg0::Arg0DispatchPaths;
use codex_core::config_loader::LoaderOverrides;
use codex_tui::Cli;


struct CodexTui;

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
}

/// Initialize backends on first use.
fn ensure_initialized() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        reqwest::backend::set_backend(wasi_http_backend::WasiHttpBackend);
        tokio::process_backend::set_backend(shell_exec_backend::WasiShellBackend);
        tokio::websocket_backend::set_backend(websocket_backend::WasiWebSocketBackend);
        tokio::set_yield_fn(wasm_yield);
        // Register webbrowser shim → WIT browser binding
        webbrowser::set_open_handler(|url| {
            bindings::host::browser::actions::open_url(url).map_err(|e| e.to_string())
        });
    });
}

impl Guest for CodexTui {
    fn run() -> i32 {
        ensure_initialized();

        console_log::console_log!("[codex-wasm-tui] run() entered, calling block_on");
        tokio::block_on(async {
            console_log::console_log!("[codex-wasm-tui] block_on started, yielding then creating Cli");
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
            match codex_tui::run_main(cli, arg0_paths, LoaderOverrides::default(), None, None).await {
                Ok(exit_info) => {
                    match exit_info.exit_reason {
                        codex_tui::ExitReason::UserRequested => 0,
                        codex_tui::ExitReason::Fatal(_) => 1,
                    }
                }
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
