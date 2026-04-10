//! String replacement transforms that cannot be eliminated by shim crates.
//!
//! These inject code that doesn't correspond to any importable crate API
//! (connector function stubs, timezone workarounds).

use crate::transform::Transform;

pub fn transforms() -> Vec<Transform> {
    vec![
        // Add missing connector functions that were previously in codex_chatgpt::connectors.
        // After the codemod rewrites codex_chatgpt → codex_core, the app-server expects
        // these functions in codex_core::connectors. They are stubs since the chatgpt
        // auth layer is not available in WASM.
        Transform::ReplaceFirst {
            path_suffix: "core/src/connectors.rs",
            find: "\n#[cfg(test)]",
            replace: r#"
// [codex-codemod-connector-stubs] — injected stubs for WASM build
/// List all connectors with options — stub for WASM (no ChatGPT auth).
/// Previously in codex_chatgpt::connectors, moved to core after codemod.
pub async fn list_all_connectors_with_options(
    _config: &crate::config::Config,
    _force_refetch: bool,
) -> anyhow::Result<Vec<AppInfo>> {
    Ok(Vec::new())
}

/// List cached connectors — stub for WASM (no ChatGPT auth).
/// Previously in codex_chatgpt::connectors, moved to core after codemod.
pub async fn list_cached_all_connectors(
    _config: &crate::config::Config,
) -> Option<Vec<AppInfo>> {
    Some(Vec::new())
}

/// Filter connectors for plugin apps — stub for WASM.
/// Previously in codex_chatgpt::connectors, moved to core after codemod.
pub fn connectors_for_plugin_apps(
    connectors: Vec<AppInfo>,
    plugin_apps: &[crate::plugins::AppConnectorId],
) -> Vec<AppInfo> {
    let plugin_app_ids: std::collections::HashSet<&str> = plugin_apps
        .iter()
        .map(|connector_id| connector_id.0.as_str())
        .collect();
    filter_disallowed_connectors(merge_plugin_apps(connectors, plugin_apps.to_vec()))
        .into_iter()
        .filter(|connector| plugin_app_ids.contains(connector.id.as_str()))
        .collect()
}

/// Merge connectors with accessible connectors — 3-arg version for app-server.
/// The app-server code calls this as connectors::merge_connectors_with_accessible(3 args).
/// The 2-arg merge_plugin_apps_with_accessible is the upstream version used by codex-core.
pub fn merge_connectors_with_accessible(
    connectors: Vec<AppInfo>,
    accessible_connectors: Vec<AppInfo>,
    _all_connectors_loaded: bool,
) -> Vec<AppInfo> {
    let merged = merge_connectors(connectors, accessible_connectors);
    filter_disallowed_connectors(merged)
}

#[cfg(test)]"#,
        },
        // [codex-codemod-request-handle-methods] — inject notify/resolve/reject methods
        // onto InProcessAppServerRequestHandle. These methods use the same command_tx
        // channel as InProcessAppServerClient but upstream only exposes them on the client.
        // The WASM app-server needs them on the request handle since the client is consumed
        // by the event pump.
        Transform::ReplaceFirst {
            path_suffix: "app-server-client/src/lib.rs",
            find: r#"impl AppServerRequestHandle {
    pub async fn request(&self, request: ClientRequest) -> IoResult<RequestResult> {"#,
            replace: r#"impl InProcessAppServerRequestHandle {
    // [codex-codemod] injected for WASM app-server — same channel as client methods
    pub async fn notify(&self, notification: ClientNotification) -> IoResult<()> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(ClientCommand::Notify {
                notification,
                response_tx,
            })
            .await
            .map_err(|_| {
                IoError::new(
                    ErrorKind::BrokenPipe,
                    "in-process app-server worker channel is closed",
                )
            })?;
        response_rx.await.map_err(|_| {
            IoError::new(
                ErrorKind::BrokenPipe,
                "in-process app-server notify channel is closed",
            )
        })?
    }

    pub async fn resolve_server_request(
        &self,
        request_id: RequestId,
        result: JsonRpcResult,
    ) -> IoResult<()> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(ClientCommand::ResolveServerRequest {
                request_id,
                result,
                response_tx,
            })
            .await
            .map_err(|_| {
                IoError::new(
                    ErrorKind::BrokenPipe,
                    "in-process app-server worker channel is closed",
                )
            })?;
        response_rx.await.map_err(|_| {
            IoError::new(
                ErrorKind::BrokenPipe,
                "in-process app-server resolve channel is closed",
            )
        })?
    }

    pub async fn reject_server_request(
        &self,
        request_id: RequestId,
        error: JSONRPCErrorError,
    ) -> IoResult<()> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(ClientCommand::RejectServerRequest {
                request_id,
                error,
                response_tx,
            })
            .await
            .map_err(|_| {
                IoError::new(
                    ErrorKind::BrokenPipe,
                    "in-process app-server worker channel is closed",
                )
            })?;
        response_rx.await.map_err(|_| {
            IoError::new(
                ErrorKind::BrokenPipe,
                "in-process app-server reject channel is closed",
            )
        })?
    }
}

impl AppServerRequestHandle {
    pub async fn request(&self, request: ClientRequest) -> IoResult<RequestResult> {"#,
        },
        // [codex-codemod-wasm-time] - Timezone/local_time offset is not supported in WASI Preview 2.
        // Replace now_local() with now_utc() to prevent panics during session creation.
        Transform::ReplaceFirst {
            path_suffix: "rollout/src/recorder.rs",
            find: r#"    let timestamp = OffsetDateTime::now_local()
        .map_err(|e| IoError::other(format!("failed to get local time: {e}")))?;"#,
            replace: r#"    let timestamp = OffsetDateTime::now_utc();"#,
        },
        // [codex-codemod-allow-unused-imports] — suppress unused re-export warnings
        // in sse/mod.rs because our responses_websocket.rs replacement imports directly
        // from crate::sse::responses:: rather than through the re-exports.
        Transform::ReplaceFirst {
            path_suffix: "codex-api/src/sse/mod.rs",
            find: "pub(crate) use responses::ResponsesStreamEvent;",
            replace: "#[allow(unused_imports)]\npub(crate) use responses::ResponsesStreamEvent;",
        },
        Transform::ReplaceFirst {
            path_suffix: "codex-api/src/sse/mod.rs",
            find: "pub(crate) use responses::process_responses_event;",
            replace: "#[allow(unused_imports)]\npub(crate) use responses::process_responses_event;",
        },
        // [codex-codemod-allow-unused-imports] — suppress unused pub re-export warnings
        // in protocol.rs for RealtimeTranscriptDelta and RealtimeTranscriptEntry which are
        // public API re-exports but not consumed within the WASM compilation unit.
        Transform::ReplaceFirst {
            path_suffix: "codex-api/src/endpoint/realtime_websocket/protocol.rs",
            find: "pub use codex_protocol::protocol::RealtimeTranscriptDelta;",
            replace: "#[allow(unused_imports)]\npub use codex_protocol::protocol::RealtimeTranscriptDelta;",
        },
        Transform::ReplaceFirst {
            path_suffix: "codex-api/src/endpoint/realtime_websocket/protocol.rs",
            find: "pub use codex_protocol::protocol::RealtimeTranscriptEntry;",
            replace: "#[allow(unused_imports)]\npub use codex_protocol::protocol::RealtimeTranscriptEntry;",
        },
        // [codex-codemod-dedup-platform-vars] — replace cfg-gated PLATFORM_CORE_VARS
        // with a single unconditional WASM definition. The upstream has cfg(windows)
        // and cfg(unix) variants; the syn cfg-widen transforms would generate many
        // duplicate fallback definitions. Pre-empt this by replacing before syn runs.
        Transform::ReplaceFirst {
            path_suffix: "core/src/exec_env.rs",
            find: "#[cfg(target_os = \"windows\")]\nconst PLATFORM_CORE_VARS: &[&str] = &[\"PATHEXT\", \"USERNAME\", \"USERPROFILE\"];\n\n#[cfg(unix)]\nconst PLATFORM_CORE_VARS: &[&str] = &[\"HOME\", \"LANG\", \"LC_ALL\", \"LC_CTYPE\", \"LOGNAME\", \"USER\"];",
            replace: "// [codex-codemod] Single unconditional definition for WASM.\nconst PLATFORM_CORE_VARS: &[&str] = &[\"HOME\"];",
        },
    ]
}
