//! Stub-module and file-replacement transforms.
//!
//! StubModule entries blank out platform-specific test/sandbox files.
//! ReplaceFile entries swap entire files with WASM-compatible stubs
//! whose content lives under `replacements/`.

use crate::transform::Transform;

pub fn transforms() -> Vec<Transform> {
    vec![
        // ---------------------------------------------------------------
        // StubModule — blank out platform-specific test files
        // ---------------------------------------------------------------
        Transform::StubModule {
            file_name: "seatbelt_tests.rs",
        },
        Transform::StubModule {
            file_name: "landlock_tests.rs",
        },
        Transform::StubModule {
            file_name: "windows_sandbox_tests.rs",
        },
        Transform::StubModule {
            file_name: "windows_sandbox_read_grants.rs",
        },
        Transform::StubModule {
            file_name: "windows_sandbox_read_grants_tests.rs",
        },
        // ---------------------------------------------------------------
        // ReplaceFile — swap entire files with WASM-compatible stubs
        // ---------------------------------------------------------------
        //
        // Large replacements (content in external files under replacements/)
        //
        Transform::ReplaceFile {
            path_suffix: "code-mode/src/runtime/mod.rs",
            content: include_str!("../../replacements/code-mode/src/runtime/mod.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "keyring-store/src/lib.rs",
            content: include_str!("../../replacements/keyring-store/src/lib.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "network-proxy/src/lib.rs",
            content: include_str!("../../replacements/network-proxy/src/lib.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "shell-escalation/src/lib.rs",
            content: include_str!("../../replacements/shell-escalation/src/lib.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "exec-server/src/lib.rs",
            content: include_str!("../../replacements/exec-server/src/lib.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "utils/pty/src/lib.rs",
            content: include_str!("../../replacements/utils/pty/src/lib.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "terminal-detection/src/lib.rs",
            content: include_str!("../../replacements/terminal-detection/src/lib.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "codex-api/src/endpoint/realtime_websocket/mod.rs",
            content: include_str!(
                "../../replacements/codex-api/src/endpoint/realtime_websocket/mod.rs"
            ),
        },
        Transform::ReplaceFile {
            path_suffix: "codex-api/src/endpoint/realtime_websocket/methods.rs",
            content: include_str!(
                "../../replacements/codex-api/src/endpoint/realtime_websocket/methods.rs"
            ),
        },
        Transform::ReplaceFile {
            path_suffix: "codex-api/src/endpoint/realtime_websocket/protocol.rs",
            content: include_str!(
                "../../replacements/codex-api/src/endpoint/realtime_websocket/protocol.rs"
            ),
        },
        //
        // Tiny stubs (< 3 lines) — kept inline
        //
        Transform::ReplaceFile {
            path_suffix: "codex-api/src/endpoint/realtime_websocket/methods_common.rs",
            content: "//! Stub for wasip2.\n",
        },
        Transform::ReplaceFile {
            path_suffix: "codex-api/src/endpoint/realtime_websocket/methods_v1.rs",
            content: "//! Stub for wasip2.\n",
        },
        Transform::ReplaceFile {
            path_suffix: "codex-api/src/endpoint/realtime_websocket/methods_v2.rs",
            content: "//! Stub for wasip2.\n",
        },
        Transform::ReplaceFile {
            path_suffix: "codex-api/src/endpoint/realtime_websocket/protocol_common.rs",
            content: "//! Stub for wasip2.\n",
        },
        Transform::ReplaceFile {
            path_suffix: "codex-api/src/endpoint/realtime_websocket/protocol_v1.rs",
            content: "//! Stub for wasip2.\n",
        },
        Transform::ReplaceFile {
            path_suffix: "codex-api/src/endpoint/realtime_websocket/protocol_v2.rs",
            content: "//! Stub for wasip2.\n",
        },
        //
        // More large replacements
        //
        Transform::ReplaceFile {
            path_suffix: "codex-api/src/endpoint/responses_websocket.rs",
            content: include_str!(
                "../../replacements/codex-api/src/endpoint/responses_websocket.rs"
            ),
        },
        Transform::ReplaceFile {
            path_suffix: "core/src/config_loader/macos.rs",
            content: include_str!("../../replacements/core/src/config_loader/macos.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "core/src/windows_sandbox.rs",
            content: include_str!("../../replacements/core/src/windows_sandbox.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "core/src/otel_init.rs",
            content: include_str!("../../replacements/core/src/otel_init.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "core/src/seatbelt.rs",
            content: include_str!("../../replacements/core/src/seatbelt.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "core/src/landlock.rs",
            content: include_str!("../../replacements/core/src/landlock.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "rmcp-client/src/lib.rs",
            content: include_str!("../../replacements/rmcp-client/src/lib.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "core/src/file_watcher.rs",
            content: include_str!("../../replacements/core/src/file_watcher.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "core/src/plugins/startup_sync.rs",
            content: include_str!("../../replacements/core/src/plugins/startup_sync.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "package-manager/src/archive.rs",
            content: include_str!("../../replacements/package-manager/src/archive.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "codex-client/src/custom_ca.rs",
            content: include_str!("../../replacements/codex-client/src/custom_ca.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "app-server-protocol/src/export.rs",
            content: include_str!("../../replacements/app-server-protocol/src/export.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "app-server-protocol/src/schema_fixtures.rs",
            content: include_str!("../../replacements/app-server-protocol/src/schema_fixtures.rs"),
        },
        //
        // Large replacements already extracted (REPLACE_FILES_LARGE)
        //
        Transform::ReplaceFile {
            path_suffix: "apply-patch/src/invocation.rs",
            content: include_str!("../../replacements/apply-patch/src/invocation.rs"),
        },
        Transform::ReplaceFile {
            path_suffix: "shell-command/src/bash.rs",
            content: include_str!("../../replacements/shell-command/src/bash.rs"),
        },
    ]
}
