//! MCP HTTP Server
//!
//! Implements wasi:http/incoming-handler to serve MCP protocol over HTTP.
//! Also implements shell:unix/command for interactive shell mode.
//! Pure shell-based implementation without JavaScript runtime.

mod bindings;
mod http_client;
mod mcp_server;
mod shell;

// Interactive shell module
mod interactive;

use bindings::exports::shell::unix::command::ExecEnv;
use bindings::exports::shell::unix::command::Guest as CommandGuest;
use bindings::exports::wasi::http::incoming_handler::Guest as HttpGuest;
use bindings::wasi::http::types::{
    Fields, IncomingRequest, OutgoingBody, OutgoingResponse, ResponseOutparam,
};
use bindings::wasi::io::streams::{InputStream, OutputStream};
use mcp_server::{JsonRpcRequest, JsonRpcResponse, ToolResult};
use runtime_macros::mcp_tool_router;
use serde_json::json;

/// The Shell-based MCP Server (stateless, created per-request)
/// Pure shell implementation - no JavaScript runtime
///
/// Note: This struct has no state - all state is created per-request in ShellEnv.
/// We create a new instance per request to avoid RefCell borrow conflicts in sync mode,
/// where WASI calls during shell execution can trigger re-entrant behavior.
struct ShellMcpServer;

#[mcp_tool_router]
impl ShellMcpServer {
    pub fn new() -> Result<Self, String> {
        Ok(Self)
    }

    // ============================================================
    // MCP Tools - these are auto-registered by #[mcp_tool_router]
    // ============================================================

    #[mcp_tool(description = "Read the contents of a file at the given path.")]
    fn read_file(&self, path: String) -> ToolResult {
        use std::fs;

        if path.is_empty() {
            return ToolResult::error("No path provided");
        }
        match fs::read_to_string(&path) {
            Ok(content) => ToolResult::text(content),
            Err(e) => ToolResult::error(format!("Failed to read {}: {}", path, e)),
        }
    }

    #[mcp_tool(
        description = "Write content to a file at the given path. Creates parent directories if needed."
    )]
    fn write_file(&self, path: String, content: String) -> ToolResult {
        use std::fs;
        use std::path::Path;

        if path.is_empty() {
            return ToolResult::error("No path provided");
        }

        // Create parent directories if needed
        if let Some(parent) = Path::new(&path).parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = fs::create_dir_all(parent) {
                    return ToolResult::error(format!("Failed to create directories: {}", e));
                }
            }
        }

        match fs::write(&path, &content) {
            Ok(()) => ToolResult::text(format!("File written: {}", path)),
            Err(e) => ToolResult::error(format!("Failed to write {}: {}", path, e)),
        }
    }

    #[mcp_tool(description = "List files and directories at the given path.")]
    fn list(&self, path: Option<String>) -> ToolResult {
        use std::fs;

        let path = path.as_deref().unwrap_or("/");

        match fs::read_dir(path) {
            Ok(entries) => {
                let mut names: Vec<String> = Vec::new();
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        names.push(format!("{}/", name));
                    } else {
                        names.push(name);
                    }
                }
                names.sort();
                if names.is_empty() {
                    ToolResult::text("(empty directory)")
                } else {
                    ToolResult::text(names.join("\n"))
                }
            }
            Err(e) => ToolResult::error(format!("Failed to list {}: {}", path, e)),
        }
    }

    #[mcp_tool(description = "Search for a pattern in files under the given path.")]
    fn grep(&self, pattern: String, path: Option<String>) -> ToolResult {
        use std::fs;

        if pattern.is_empty() {
            return ToolResult::error("No pattern provided");
        }

        let search_path = path.as_deref().unwrap_or("/");
        let mut matches: Vec<String> = Vec::new();

        // Recursive grep implementation
        fn search_directory(
            dir_path: &str,
            pattern: &str,
            matches: &mut Vec<String>,
        ) -> Result<(), std::io::Error> {
            for entry in fs::read_dir(dir_path)? {
                let entry = entry?;
                let path = entry.path();
                let path_str = path.to_string_lossy().to_string();

                if entry.file_type()?.is_dir() {
                    // Recurse into directory
                    let _ = search_directory(&path_str, pattern, matches);
                } else if entry.file_type()?.is_file() {
                    // Search file content
                    if let Ok(content) = fs::read_to_string(&path) {
                        for (line_num, line) in content.lines().enumerate() {
                            if line.to_lowercase().contains(&pattern.to_lowercase()) {
                                let trimmed = if line.chars().count() > 100 {
                                    let end: String = line.chars().take(100).collect();
                                    format!("{}...", end)
                                } else {
                                    line.to_string()
                                };
                                matches.push(format!(
                                    "{}:{}: {}",
                                    path_str,
                                    line_num + 1,
                                    trimmed.trim()
                                ));
                            }
                        }
                    }
                }
            }
            Ok(())
        }

        if let Err(e) = search_directory(search_path, &pattern, &mut matches) {
            return ToolResult::error(format!("Grep failed: {}", e));
        }

        if matches.is_empty() {
            ToolResult::text("No matches found")
        } else {
            ToolResult::text(matches.join("\n"))
        }
    }

    #[mcp_tool(
        description = "Execute shell commands with pipe support. Supports 50+ commands including: echo, ls, cat, grep, sed, awk, jq, curl, sqlite3, tsx, tar, gzip, and more. Example: 'ls /data | head -n 5'"
    )]
    fn shell_eval(&self, command: String) -> ToolResult {
        if command.is_empty() {
            return ToolResult::error("No command provided");
        }

        let mut env = shell::ShellEnv::new();
        let result = futures_lite::future::block_on(shell::run_pipeline(&command, &mut env));

        if result.code == 0 {
            if result.stdout.is_empty() && result.stderr.is_empty() {
                ToolResult::text("(no output)")
            } else if result.stderr.is_empty() {
                ToolResult::text(result.stdout)
            } else {
                ToolResult::text(format!("{}\nstderr: {}", result.stdout, result.stderr))
            }
        } else {
            ToolResult::error(format!(
                "Exit code {}: {}{}\n",
                result.code, result.stderr, result.stdout
            ))
        }
    }

    #[mcp_tool(
        description = "Edit a file by replacing old_str with new_str. The old_str must match exactly and uniquely in the file. For multiple edits, call this tool multiple times. Use read_file first to see the current content."
    )]
    fn edit_file(&self, path: String, old_str: String, new_str: String) -> ToolResult {
        use std::fs;

        if path.is_empty() {
            return ToolResult::error("No path provided");
        }
        if old_str.is_empty() {
            return ToolResult::error(
                "old_str cannot be empty (use write_file for creating new files)",
            );
        }

        // Read the current file content
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => return ToolResult::error(format!("Failed to read {}: {}", path, e)),
        };

        // Count occurrences of old_str
        let count = content.matches(&old_str).count();

        if count == 0 {
            // Provide helpful context about what's in the file
            let preview_lines: Vec<&str> = content.lines().take(10).collect();
            let preview = preview_lines.join("\n");
            return ToolResult::error(format!(
                "old_str not found in file.\n\nFirst 10 lines of file:\n{}\n\nMake sure old_str matches exactly (including whitespace).",
                preview
            ));
        }

        if count > 1 {
            // Find line numbers where matches occur to help user be more specific
            let mut match_lines = Vec::new();
            for (line_num, line) in content.lines().enumerate() {
                if line.contains(&old_str) {
                    match_lines.push(format!(
                        "  Line {}: {}",
                        line_num + 1,
                        if line.chars().count() > 60 {
                            let end: String = line.chars().take(60).collect();
                            format!("{}...", end)
                        } else {
                            line.to_string()
                        }
                    ));
                }
            }
            return ToolResult::error(format!(
                "old_str found {} times. Include more context to make it unique.\n\nMatches at:\n{}",
                count,
                match_lines.join("\n")
            ));
        }

        // Perform the replacement (exactly one match)
        let new_content = content.replacen(&old_str, &new_str, 1);

        // Write the updated content
        match fs::write(&path, &new_content) {
            Ok(()) => {
                let old_lines = old_str.lines().count();
                let new_lines = new_str.lines().count();
                ToolResult::text(format!(
                    "Edited {}: replaced {} line(s) with {} line(s)",
                    path, old_lines, new_lines
                ))
            }
            Err(e) => ToolResult::error(format!("Failed to write {}: {}", path, e)),
        }
    }
}

/// Handle JSON-RPC request
///
/// Creates a fresh ShellMcpServer instance per request to avoid RefCell borrow
/// conflicts in sync mode (Safari). The server is stateless, so this is safe.
fn handle_mcp_request(request_str: &str) -> String {
    match serde_json::from_str::<JsonRpcRequest>(request_str) {
        Ok(req) => {
            let mut server = ShellMcpServer::new().expect("Failed to create MCP server");
            let response = mcp_server::handle_request(&mut server, req);
            serde_json::to_string(&response)
                .unwrap_or_else(|_| r#"{"error":"serialize failed"}"#.to_string())
        }
        Err(e) => {
            let err = JsonRpcResponse::error(None, -32700, format!("Parse error: {}", e));
            serde_json::to_string(&err)
                .unwrap_or_else(|_| r#"{"error":"serialize failed"}"#.to_string())
        }
    }
}

/// Handle OAuth callback — receives authorization code from browser redirect.
/// Reads pending login state from `.login_pending.json` on OPFS (written by
/// codex-tui's `run_login_server()`), exchanges the code for tokens, saves
/// `auth.json`, and removes the pending file to signal completion.
///
/// Returns (body, content_type, status_code).
fn handle_oauth_callback(path: &str) -> (String, &'static str, u16) {
    // Parse query parameters from path
    let query = path.splitn(2, '?').nth(1).unwrap_or("");
    let params: std::collections::HashMap<&str, &str> = query
        .split('&')
        .filter_map(|pair| {
            let mut kv = pair.splitn(2, '=');
            Some((kv.next()?, kv.next().unwrap_or("")))
        })
        .collect();

    let state = params.get("state").copied().unwrap_or("");
    let code = params.get("code").copied();
    let error = params.get("error").copied();

    if state.is_empty() {
        return (
            html_page("Login Failed", "Missing state parameter."),
            "text/html",
            400,
        );
    }

    // Find the pending login file by scanning known codex home locations
    let pending = match find_pending_login(state) {
        Some(p) => p,
        None => {
            return (
                html_page("Login Failed", "Unknown or expired login session."),
                "text/html",
                400,
            );
        }
    };

    if let Some(err) = error {
        let desc = params.get("error_description").copied().unwrap_or(err);
        let _ = std::fs::remove_file(&pending.path);
        return (
            html_page("Login Failed", &format!("Authentication error: {}", desc)),
            "text/html",
            400,
        );
    }

    match code {
        Some(code) => {
            match exchange_code_for_token(&pending, code) {
                Ok(()) => {
                    // Remove pending file to signal completion to block_until_done()
                    let _ = std::fs::remove_file(&pending.path);
                    (
                        html_page(
                            "Login Successful",
                            "You can close this tab and return to the terminal.",
                        ),
                        "text/html",
                        200,
                    )
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&pending.path);
                    (
                        html_page("Login Failed", &format!("Token exchange failed: {}", e)),
                        "text/html",
                        500,
                    )
                }
            }
        }
        None => (
            html_page("Login Failed", "Missing authorization code."),
            "text/html",
            400,
        ),
    }
}

/// Pending login info read from `.login_pending.json`.
struct PendingLoginInfo {
    code_verifier: String,
    client_id: String,
    redirect_uri: String,
    codex_home: String,
    path: std::path::PathBuf,
}

/// Search known codex home locations for a pending login matching the state.
fn find_pending_login(state: &str) -> Option<PendingLoginInfo> {
    // Check common codex home locations on OPFS
    let candidates = [
        std::path::PathBuf::from("/home/user/.codex"),
        std::path::PathBuf::from("/home/user/.config/codex"),
    ];

    // Also check CODEX_HOME env var if set
    let mut search_paths: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(home) = std::env::var("CODEX_HOME") {
        search_paths.push(std::path::PathBuf::from(home));
    }
    search_paths.extend(candidates);

    for dir in &search_paths {
        let pending_path = dir.join(".login_pending.json");
        if let Ok(contents) = std::fs::read_to_string(&pending_path) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&contents) {
                if v["state"].as_str() == Some(state) {
                    return Some(PendingLoginInfo {
                        code_verifier: v["code_verifier"].as_str().unwrap_or("").to_string(),
                        client_id: v["client_id"].as_str().unwrap_or("").to_string(),
                        redirect_uri: v["redirect_uri"].as_str().unwrap_or("").to_string(),
                        codex_home: dir.to_string_lossy().to_string(),
                        path: pending_path,
                    });
                }
            }
        }
    }
    None
}

/// Exchange an OAuth authorization code for an access token and save credentials.
fn exchange_code_for_token(pending: &PendingLoginInfo, code: &str) -> Result<(), String> {
    let token_url = "https://auth.openai.com/oauth/token";

    let body = serde_json::json!({
        "grant_type": "authorization_code",
        "client_id": pending.client_id,
        "code": code,
        "redirect_uri": pending.redirect_uri,
        "code_verifier": pending.code_verifier,
    });

    let body_str = serde_json::to_string(&body).map_err(|e| e.to_string())?;
    let headers = serde_json::json!({"content-type": "application/json"});
    let headers_str = serde_json::to_string(&headers).map_err(|e| e.to_string())?;

    let resp = http_client::fetch_request("POST", token_url, Some(&headers_str), Some(&body_str))
        .map_err(|e| format!("Token request failed: {}", e))?;

    if !resp.ok {
        return Err(format!(
            "Token endpoint returned {}: {}",
            resp.status,
            resp.text_lossy()
        ));
    }

    // Parse token response
    let token_resp: serde_json::Value = serde_json::from_str(&resp.text_lossy())
        .map_err(|e| format!("Parse token response: {}", e))?;

    let access_token = token_resp["access_token"]
        .as_str()
        .ok_or("Missing access_token in response")?;

    // Save auth.json
    let home = std::path::Path::new(&pending.codex_home);
    std::fs::create_dir_all(home).map_err(|e| e.to_string())?;

    let auth_json = serde_json::json!({
        "auth_mode": "apikey",
        "openai_api_key": access_token,
        "tokens": token_resp,
    });
    let json_str = serde_json::to_string_pretty(&auth_json).map_err(|e| e.to_string())?;
    std::fs::write(home.join("auth.json"), json_str).map_err(|e| e.to_string())?;

    Ok(())
}

/// Generate a simple HTML page for browser display.
fn html_page(title: &str, message: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html><head><title>{title}</title>
<style>body{{font-family:system-ui;display:flex;justify-content:center;align-items:center;height:100vh;margin:0;background:#1a1a2e;color:#e0e0e0}}
.card{{background:#16213e;border-radius:12px;padding:2em 3em;text-align:center;box-shadow:0 4px 20px rgba(0,0,0,.3)}}</style>
</head><body><div class="card"><h1>{title}</h1><p>{message}</p></div></body></html>"#,
    )
}

/// HTTP Component
struct Component;

bindings::export!(Component with_types_in bindings);

impl HttpGuest for Component {
    fn handle(request: IncomingRequest, outparam: ResponseOutparam) {
        // Get request headers and path
        let headers = request.headers();
        let path = request.path_with_query().unwrap_or_default();

        // Read request body
        let body = request.consume().expect("consume body");
        let stream = body.stream().expect("get stream");

        let mut request_bytes = Vec::new();
        loop {
            match stream.blocking_read(65536) {
                Ok(bytes) if bytes.is_empty() => break,
                Ok(bytes) => request_bytes.extend(bytes),
                Err(_) => break,
            }
        }
        drop(stream);
        bindings::wasi::http::types::IncomingBody::finish(body);

        // Check for SSE support in Accept header
        let accept_sse = headers.entries().iter().any(|(k, v)| {
            k.to_lowercase() == "accept" && String::from_utf8_lossy(v).contains("text/event-stream")
        });

        // Simple endpoint routing
        let (response_body, content_type, status_code) = if path.starts_with("/oauth/callback") {
            // OAuth callback — extract code and state from query params
            let result = handle_oauth_callback(&path);
            (result.0, result.1, result.2)
        } else if path.starts_with("/sse") && accept_sse {
            // SSE endpoint - establish connection
            (
                handle_sse_connection(&request_bytes),
                "text/event-stream",
                200u16,
            )
        } else {
            // JSON-RPC endpoint
            let request_str = String::from_utf8_lossy(&request_bytes);
            (handle_mcp_request(&request_str), "application/json", 200u16)
        };

        // Prepare response headers
        let hdrs = Fields::new();

        if content_type == "text/event-stream" {
            hdrs.set(
                &"content-type".to_string(),
                &[b"text/event-stream".to_vec()],
            )
            .ok();
            hdrs.set(&"cache-control".to_string(), &[b"no-cache".to_vec()])
                .ok();
            hdrs.set(&"connection".to_string(), &[b"keep-alive".to_vec()])
                .ok();
        } else {
            hdrs.set(
                &"content-type".to_string(),
                &[content_type.as_bytes().to_vec()],
            )
            .ok();
        }

        hdrs.set(&"access-control-allow-origin".to_string(), &[b"*".to_vec()])
            .ok();

        // Send response
        let resp = OutgoingResponse::new(hdrs);
        resp.set_status_code(status_code).ok();

        let body = resp.body().expect("response body");
        ResponseOutparam::set(outparam, Ok(resp));

        let out = body.write().expect("write stream");
        // Write in chunks — blocking_write_and_flush is limited to 4096 bytes per WASI spec
        let response_bytes = response_body.as_bytes();
        let mut offset = 0;
        while offset < response_bytes.len() {
            let end = (offset + 4096).min(response_bytes.len());
            out.blocking_write_and_flush(&response_bytes[offset..end])
                .expect("write");
            offset = end;
        }
        drop(out);
        OutgoingBody::finish(body, None).unwrap();
    }
}

/// Handle SSE connection for MCP streaming protocol
fn handle_sse_connection(_request_bytes: &[u8]) -> String {
    // For SSE, we send events in the format:
    // event: message\ndata: {...}\n\n

    // Send initialization event
    let init_event = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {}
    });

    format!("event: message\ndata: {}\n\n", init_event)
}

// cdylib component entry point - exports handle the actual entry point

/// Interactive shell implementation
impl CommandGuest for Component {
    fn run(
        name: String,
        args: Vec<String>,
        env: ExecEnv,
        stdin: InputStream,
        stdout: OutputStream,
        stderr: OutputStream,
    ) -> i32 {
        match name.as_str() {
            "sh" | "shell" | "bash" | "brush-shell" => {
                interactive::run_shell(args, env, stdin, stdout, stderr)
            }
            _ => {
                let msg = format!("Unknown command: {}\n", name);
                let _ = stderr.blocking_write_and_flush(msg.as_bytes());
                127
            }
        }
    }

    fn list_commands() -> Vec<String> {
        vec![
            "sh".to_string(),
            "shell".to_string(),
            "bash".to_string(),
            "brush-shell".to_string(),
        ]
    }
}
