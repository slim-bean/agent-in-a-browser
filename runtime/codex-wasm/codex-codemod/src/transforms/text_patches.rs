//! Nonsemantic file/path text patches.
//!
//! These are formatting-preserving string edits that do not require syntax or
//! symbol resolution. They previously lived inside the monolithic syn-based
//! global pass and are now an explicit phase before semantic analysis.

use crate::transform::Transform;

pub fn transforms() -> Vec<Transform> {
    let mut transforms = Vec::new();

    for (path_suffix, text) in [
        (
            "code-mode/src/lib.rs",
            "#![allow(unreachable_code, unused_variables, unused_mut, dead_code, unused_imports, unused_assignments)]\n",
        ),
        (
            "state/src/lib.rs",
            "#![allow(unused_variables, unused_mut, unused_imports, dead_code)]\n",
        ),
        (
            "hooks/src/lib.rs",
            "#![allow(unused_variables, unused_mut, unused_imports)]\n",
        ),
        (
            "app-server-protocol/src/lib.rs",
            "#![allow(dead_code, unused_imports)]\n",
        ),
        (
            "codex-client/src/lib.rs",
            "#![allow(dead_code, unused_imports)]\n",
        ),
        (
            "shell-command/src/lib.rs",
            "#![allow(dead_code, unused_variables, unused_imports)]\n",
        ),
        (
            "file-search/src/lib.rs",
            "#![allow(unused_imports, dead_code, unused_variables, unreachable_code)]\n",
        ),
        (
            "core/src/lib.rs",
            "#![allow(unreachable_code, unused_variables, unused_mut, dead_code, unused_imports, unused_assignments)]\n",
        ),
        (
            "async-utils/src/lib.rs",
            "#![allow(unused_variables, unused_imports)]\n",
        ),
        (
            "apply-patch/src/lib.rs",
            "#![allow(unused_variables, unused_imports)]\n",
        ),
        (
            "arg0/src/lib.rs",
            "#![allow(dead_code, unused_variables, unused_imports)]\n",
        ),
        (
            "feedback/src/lib.rs",
            "#![allow(dead_code, unused_imports)]\n",
        ),
        ("rollout/src/lib.rs", "#![allow(unused_imports)]\n"),
        (
            "app-server/src/lib.rs",
            "#![allow(unused_variables, unused_mut, unused_assignments, dead_code)]\n",
        ),
        (
            "app-server-client/src/lib.rs",
            "#![allow(unused_variables, unused_mut, unused_assignments)]\n",
        ),
        (
            "tui/src/lib.rs",
            "#![allow(unexpected_cfgs, unused_imports, unused_variables, unused_mut, dead_code, unused_assignments, unused_attributes)]\n",
        ),
    ] {
        transforms.push(Transform::Prepend { path_suffix, text });
    }

    transforms.push(Transform::CommentOutLines {
        path_suffix: "core/src/config_loader/macos.rs",
        prefixes: &["use core_foundation::"],
    });

    transforms.extend([
        Transform::ReplaceFirst {
            path_suffix: "core/src/codex.rs",
            find: "            auth_mode,\n            originator.clone(),\n            config.otel.log_user_prompt,",
            replace: "            auth_mode.clone(),\n            originator.clone(),\n            config.otel.log_user_prompt,",
        },
        Transform::ReplaceFirst {
            path_suffix: "package-manager/src/manager.rs",
            find: "use fd_lock::RwLock as FileRwLock;",
            replace: "/// Stub for fd_lock::RwLock (stripped for WASM)\nstruct FileRwLock<T>(T);\nimpl<T> FileRwLock<T> {\n    fn new(inner: T) -> Self { Self(inner) }\n    fn try_write(&mut self) -> std::io::Result<&mut T> { Ok(&mut self.0) }\n}",
        },
        Transform::ReplaceFirst {
            path_suffix: "codex-client/src/transport.rs",
            find: "                        RequestCompression::Zstd => (\n                            zstd::stream::encode_all(std::io::Cursor::new(json), 3)\n                                .map_err(|err| TransportError::Build(err.to_string()))?,\n                            http::HeaderValue::from_static(\"zstd\"),\n                        ),",
            replace: "                        RequestCompression::Zstd => (\n                            json,\n                            http::HeaderValue::from_static(\"identity\"),\n                        ),",
        },
        Transform::ReplaceFirst {
            path_suffix: "core/src/skills/remote.rs",
            find: "    let cursor = std::io::Cursor::new(bytes);\n    let mut archive = zip::ZipArchive::new(cursor).context(\"Failed to open zip archive\")?;\n    for i in 0..archive.len() {\n        let mut file = archive.by_index(i).context(\"Failed to read zip entry\")?;\n        if file.is_dir() {\n            continue;\n        }\n        let raw_name = file.name().to_string();\n        let normalized = normalize_zip_name(&raw_name, prefix_candidates);\n        let Some(normalized) = normalized else {\n            continue;\n        };\n        let file_path = safe_join(output_dir, &normalized)?;\n        if let Some(parent) = file_path.parent() {\n            std::fs::create_dir_all(parent)\n                .with_context(|| format!(\"Failed to create parent dir for {normalized}\"))?;\n        }\n        let mut out = std::fs::File::create(&file_path)\n            .with_context(|| format!(\"Failed to create file {normalized}\"))?;\n        std::io::copy(&mut file, &mut out)\n            .with_context(|| format!(\"Failed to write skill file {normalized}\"))?;\n    }\n    Ok(())",
            replace: "    let _ = (bytes, output_dir, prefix_candidates);\n    anyhow::bail!(\"zip extraction not available in WASM\")",
        },
        Transform::ReplaceFirst {
            path_suffix: "arg0/src/lib.rs",
            find: "    if exe_name == CODEX_LINUX_SANDBOX_ARG0 {\n        // Safety: [`run_main`] never returns.\n        codex_linux_sandbox::run_main();\n    } else if exe_name == APPLY_PATCH_ARG0 || exe_name == MISSPELLED_APPLY_PATCH_ARG0",
            replace: "    #[cfg(not(target_arch = \"wasm32\"))]\n    if exe_name == CODEX_LINUX_SANDBOX_ARG0 {\n        // Safety: [`run_main`] never returns.\n        codex_linux_sandbox::run_main();\n    }\n    if exe_name == APPLY_PATCH_ARG0 || exe_name == MISSPELLED_APPLY_PATCH_ARG0",
        },
        Transform::ReplaceFirst {
            path_suffix: "arg0/src/lib.rs",
            find: "    builder.thread_stack_size(TOKIO_WORKER_STACK_SIZE_BYTES);",
            replace: "    #[cfg(not(target_arch = \"wasm32\"))]\n    builder.thread_stack_size(TOKIO_WORKER_STACK_SIZE_BYTES);",
        },
        Transform::ReplaceFirst {
            path_suffix: "file-search/src/lib.rs",
            find: ") -> anyhow::Result<FileSearchSession> {\n    let FileSearchOptions {",
            replace: ") -> anyhow::Result<FileSearchSession> {\n    #[cfg(target_arch = \"wasm32\")]\n    {\n        let _ = (&search_directories, &options, &reporter, &cancel_flag);\n        anyhow::bail!(\"File search is not available in the browser (requires OS threads)\");\n    }\n    let FileSearchOptions {",
        },
        Transform::ReplaceFirst {
            path_suffix: "tui/src/tui.rs",
            find: "    if !stdin().is_terminal() {\n        return Err(std::io::Error::other(\"stdin is not a terminal\"));\n    }\n    if !stdout().is_terminal() {\n        return Err(std::io::Error::other(\"stdout is not a terminal\"));\n    }",
            replace: "    // [codex-codemod] is_terminal() checks skipped — WASM stdin/stdout are ghostty-web terminal",
        },
        Transform::ReplaceFirst {
            path_suffix: "utils/home-dir/src/lib.rs",
            find: "                path.canonicalize().map_err(|err| {\n                    std::io::Error::new(\n                        err.kind(),\n                        format!(\"failed to canonicalize CODEX_HOME {val:?}: {err}\"),\n                    )\n                })",
            replace: "                Ok(path)",
        },
        Transform::ReplaceFirst {
            path_suffix: "app-server/src/codex_message_processor.rs",
            find: "let response = FeedbackUploadResponse { thread_id };",
            replace: "let response = FeedbackUploadResponse { thread_id: thread_id.unwrap_or_default() };",
        },
        Transform::ReplaceFirst {
            path_suffix: "tui/src/app.rs",
            find: "/*account_id*/ None,\n            bootstrap.account_email.clone(),",
            replace: "/*account_id*/ None::<String>,\n            bootstrap.account_email.clone(),",
        },
        Transform::ReplaceFirst {
            path_suffix: "tui/src/chatwidget.rs",
            find: "            snapshot.feedback_diagnostics(),\n        );\n        self.bottom_pane.show_selection_view(params);",
            replace: "            &snapshot.feedback_diagnostics(),\n        );\n        self.bottom_pane.show_selection_view(params);",
        },
        Transform::ReplaceFirst {
            path_suffix: "git-utils/src/info.rs",
            find: "async fn run_git_command_with_timeout(args: &[&str], cwd: &Path) -> Option<std::process::Output> {",
            replace: "async fn run_git_command_with_timeout(args: &[&str], cwd: &Path) -> Option<tokio::process::Output> {",
        },
        Transform::ReplaceFirst {
            path_suffix: "tui/src/clipboard_text.rs",
            find: "    let error = match arboard::Clipboard::new() {\n        Ok(mut clipboard) => match clipboard.set_text(text.to_string()) {\n            Ok(()) => return Ok(()),\n            Err(err) => format!(\"clipboard unavailable: {err}\"),\n        },\n        Err(err) => format!(\"clipboard unavailable: {err}\"),\n    };",
            replace: "    let error = \"clipboard not available in WASM\".to_string();",
        },
        Transform::ReplaceFirst {
            path_suffix: "tui/src/tooltips.rs",
            find: "        let client = reqwest::blocking::Client::builder()\n            .no_proxy()\n            .build()\n            .ok()?;\n        let response = client\n            .get(ANNOUNCEMENT_TIP_URL)\n            .timeout(Duration::from_millis(2000))\n            .send()\n            .ok()?;\n        response.error_for_status().ok()?.text().ok()",
            replace: "        // reqwest::blocking not available in WASM\n        None::<String>",
        },
        Transform::ReplaceFirst {
            path_suffix: "login/src/server.rs",
            find: "use std::thread;\n",
            replace: "",
        },
        Transform::ReplaceFirst {
            path_suffix: "login/src/server.rs",
            find: "let redirect_uri = format!(\"http://localhost:{actual_port}/auth/callback\");",
            replace: "let redirect_uri = std::env::var(\"CODEX_REDIRECT_URI\")\n            .unwrap_or_else(|_| format!(\"http://localhost:{actual_port}/auth/callback\"));",
        },
        Transform::ReplaceFirst {
            path_suffix: "core/src/codex.rs",
            find: ".map(TelemetryAuthMode::from)",
            replace: ".map(|m| TelemetryAuthMode::from_display(&m))",
        },
        Transform::ReplaceFirst {
            path_suffix: "core/src/models_manager/manager.rs",
            find: "TelemetryAuthMode::from(mode)",
            replace: "TelemetryAuthMode::from_display(&mode)",
        },
        Transform::ReplaceFirst {
            path_suffix: "models-manager/src/manager.rs",
            find: "TelemetryAuthMode::from(mode)",
            replace: "TelemetryAuthMode::from_display(&mode)",
        },
        Transform::ReplaceFirst {
            path_suffix: "tui/src/chatwidget.rs",
            find: "connectors::list_all_connectors_with_options(&config, force_refetch).await?",
            replace: "{ let _ = (&config, force_refetch); Vec::<connectors::AppInfo>::new() }",
        },
        Transform::ReplaceFirst {
            path_suffix: "tui/src/chatwidget.rs",
            find: "merge_connectors_with_accessible(\n                    all_connectors,\n                    accessible_connectors,\n                    /*all_connectors_loaded*/ true,\n                )",
            replace: "merge_plugin_apps_with_accessible(\n                    Vec::new(),\n                    accessible_connectors,\n                )",
        },
        Transform::ReplaceFirst {
            path_suffix: "tui/src/chatwidget.rs",
            find: "merge_connectors_with_accessible(\n                        Vec::new(),\n                        snapshot.connectors,\n                        /*all_connectors_loaded*/ false,\n                    )",
            replace: "merge_plugin_apps_with_accessible(\n                        Vec::new(),\n                        snapshot.connectors,\n                    )",
        },
    ]);

    transforms
}
