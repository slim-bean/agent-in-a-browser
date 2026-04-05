//! Cargo.toml transforms — the primary codemod mechanism.
//!
//! The key insight: by redirecting `tokio`, `reqwest`, and `crossterm`
//! dependencies to our shim crates via path overrides, upstream .rs files
//! need zero changes for these transforms.

use anyhow::{Context, Result};
use std::path::Path;
use toml_edit::{DocumentMut, Item, Value};
use walkdir::WalkDir;

/// Dependencies to redirect to local fork submodules.
/// Format: (dep_name, relative_path_from_workspace_root).
const SUBMODULE_REDIRECTS: &[(&str, &str)] = &[
    ("tree-sitter", "../../../tree-sitter-wasm/tree-sitter/lib"),
    (
        "tree-sitter-bash",
        "../../../tree-sitter-wasm/tree-sitter-bash",
    ),
];

/// Patches to inject into [patch.crates-io] to unify transitive dependencies.
/// Format: (dep_name, relative_path_from_workspace_root).
const INJECT_PATCHES: &[(&str, &str)] = &[
    // Ensure tree-sitter-bash uses the same tree-sitter-language as our local tree-sitter,
    // avoiding diamond dependency (different crate instances for the same type).
    (
        "tree-sitter-language",
        "../../../tree-sitter-wasm/tree-sitter/lib/language",
    ),
];

/// Dependencies to redirect to our shim crates.
const SHIM_REDIRECTS: &[(&str, &str)] = &[
    ("tokio", "wasi-tokio"),
    ("tokio-util", "wasi-tokio-util"),
    ("reqwest", "wasi-reqwest"),
    ("crossterm", "wasi-crossterm"),
    ("codex-otel", "wasi-codex-otel"),
    // codex-login: use upstream (compiles for WASM as-is via shimmed tiny_http/reqwest/tokio)
    // libc: use real crate (0.2.x has wasm32-wasip2 support)
    ("os_info", "wasi-os-info"),
    ("codex-keyring-store", "wasi-keyring-store"),
    ("codex-terminal-detection", "wasi-terminal-detection"),
    ("codex-shell-escalation", "wasi-shell-escalation"),
    ("codex-utils-pty", "wasi-pty"),
    ("codex-network-proxy", "wasi-network-proxy"),
    ("codex-exec-server", "wasi-exec-server"),
    ("codex-rmcp-client", "wasi-rmcp-client"),
    ("cpal", "wasi-cpal"),
];

/// Per-crate additional deps to strip (crate_dir_name → deps to strip).
/// These are stripped ON TOP OF the global STRIP_DEPS list.
const PER_CRATE_STRIP_DEPS: &[(&str, &[&str])] = &[
    // rmcp-client: migrated to wasi-rmcp-client shim crate
    // network-proxy: migrated to wasi-network-proxy shim crate
    // shell-escalation: migrated to wasi-shell-escalation shim crate
    // exec-server: migrated to wasi-exec-server shim crate
    // pty: migrated to wasi-pty shim crate
    // ("login", &["tiny_http"]),  // tiny_http now shimmed via wasi-tiny-http
    (
        "app-server",
        &[
            "axum",
            "jsonwebtoken",
            "constant_time_eq",
            "hmac",
            "sha2",
            "codex-backend-client",
            "codex-cloud-requirements",
        ],
    ),
    ("app-server-client", &["tokio-tungstenite", "tungstenite"]),
    ("core", &["notify"]),
    ("tui", &["hound"]),
    // state: sqlx is now redirected to wasi-sqlx via [patch.crates-io]
    // ("state", &["sqlx"]),
];

/// rmcp features to strip from any crate's rmcp dependency.
/// These transport features pull in process-wrap, real reqwest, etc.
const STRIP_RMCP_FEATURES: &[&str] = &["auth", "transport-streamable-http-server"];

/// Dependencies that were originally under [target.'cfg(unix)'.dependencies] etc.
/// but need to be moved to [dependencies] since we strip all [target] sections.
/// Format: (crate_dir_name, dep_name)
const RESCUE_TARGET_DEPS: &[(&str, &str)] = &[("core", "codex-shell-escalation")];

/// Per-crate deps to inject (crate_dir_name → (dep_name, relative_path_from_crate)).
/// These deps are used by the TUI source but aren't in its original Cargo.toml.
const INJECT_DEPS: &[(&str, &[(&str, &str)])] = &[
    (
        "tui",
        &[
            ("codex-feedback", "../../../codex-wasm/wasi-codex-feedback"),
            ("codex-arg0", "../arg0"),
            ("codex-utils-sleep-inhibitor", "../utils/sleep-inhibitor"),
            ("arboard", "../../../codex-wasm/wasi-arboard"),
            ("cpal", "../../../codex-wasm/wasi-cpal"),
            ("console-log", "../../../crates/console-log"),
        ],
    ),
    ("core", &[("console-log", "../../../crates/console-log")]),
    (
        "app-server",
        &[
            ("codex-feedback", "../../../codex-wasm/wasi-codex-feedback"),
            ("console-log", "../../../crates/console-log"),
        ],
    ),
    (
        "app-server-client",
        &[("codex-feedback", "../../../codex-wasm/wasi-codex-feedback")],
    ),
    (
        "code-mode",
        &[("wasi-js-engine", "../../../crates/wasi-js-engine")],
    ),
    // state: sqlx is stripped globally but state's runtime.rs uses it;
    // inject the wasi-sqlx shim path so it resolves via [patch.crates-io]
    ("state", &[("sqlx", "../../../codex-wasm/wasi-sqlx")]),
    (
        "code-mode",
        &[("wasi-code-runtime", "../../../codex-wasm/wasi-code-runtime")],
    ),
];

/// Dependencies to strip entirely (platform-specific or from removed members).
const STRIP_DEPS: &[&str] = &[
    // Platform-specific external deps
    "keyring",
    "landlock",
    "seccompiler",
    // libc: redirected to wasi-libc shim
    "openssl-sys",
    "portable-pty",
    // arboard: redirected to wasi-arboard shim (not stripped)
    // webbrowser: redirected to wasi-webbrowser shim (not stripped)
    // cpal: redirected to wasi-cpal shim (not stripped)
    "hound",
    "windows-sys",
    "winsplit",
    "uds_windows",
    "sentry",
    // Internal crates from stripped workspace members
    "codex-windows-sandbox",
    "codex-linux-sandbox",
    "codex-process-hardening",
    // codex-network-proxy: redirected to wasi-network-proxy shim via SHIM_REDIRECTS
    // codex-otel and codex-login: redirected to shim crates, not stripped
    "codex-lmstudio",
    "codex-ollama",
    // codex-terminal-detection: redirected to wasi-terminal-detection shim via SHIM_REDIRECTS
    // codex-utils-pty: redirected to wasi-pty shim via SHIM_REDIRECTS
    // codex-utils-sleep-inhibitor: kept — used by TUI (no-op dummy backend on WASM)
    "codex-chatgpt",
    "codex-feedback",
    "codex-backend-client",
    "codex-backend-openapi-models",
    "codex-cloud-requirements",
    "codex-cloud-tasks",
    "codex-cloud-tasks-client",
    "codex-stdio-to-uds",
    "codex-debug-client",
    "codex-v8-poc",
    "codex-utils-oss",
    "codex-tui-app-server",
    "codex-tui",
    "codex-cli",
    "codex-exec",
    // codex-arg0: kept — used by TUI for Arg0DispatchPaths
    "codex-mcp-server",
    // codex-exec-server: redirected to wasi-exec-server shim via SHIM_REDIRECTS
    // codex-app-server: kept — in-process path works in WASM
    // codex-app-server-client: kept — provides AppServerClient types
    "codex-app-server-test-client",
    "codex-responses-api-proxy",
    // Test support crates
    "app_test_support",
    "core_test_support",
    "mcp_test_support",
    // Non-essential external deps that are problematic in WASM
    "sentry",
    "v8",
    // cpal: redirected to wasi-cpal shim via SHIM_REDIRECTS (not stripped)
    "hound",
    // Websocket deps — use OpenAI patched forks with "proxy" feature
    // that aren't available from crates.io. Websocket support is fully stubbed.
    "tokio-tungstenite",
    "tungstenite",
    // path-absolutize/path-dedot don't compile for wasm32 (missing trait impls)
    "path-absolutize",
    // ts-rs: TypeScript binding generator, not needed in WASM
    "ts-rs",
    // TLS/crypto — not needed in WASM (HTTP handled by host via wasi:http)
    "rustls",
    "rustls-native-certs",
    "rustls-pki-types",
    "rustls-pemfile",
    "webpki-roots",
    // sqlx: stripped from non-TUI crates; TUI gets it via wasi-sqlx [patch.crates-io]
    "sqlx",
    // Rustls provider — not needed (HTTP via wasi:http)
    "codex-utils-rustls-provider",
    // hyper-rustls — not needed (HTTP via wasi:http)
    "hyper-rustls",
    // zstd — C library, doesn't compile for wasm32-wasip2
    "zstd",
    // zip — depends on zstd, artifact handling done by host
    "zip",
    // which — uses unstable wasip2 std::os::wasi feature
    "which",
    // tree-sitter — redirected to local fork submodules (see SUBMODULE_REDIRECTS)
    // "tree-sitter",
    // "tree-sitter-bash",
    // flate2/tar — archive deps (archive handling done by host)
    "flate2",
    "tar",
    // fd-lock — OS-specific file locking, doesn't compile for wasm32
    "fd-lock",
];

/// Crates to KEEP in workspace members. Everything else is stripped.
/// This is an allowlist — much safer than a denylist since upstream
/// adds new crates frequently.
const KEEP_WORKSPACE_MEMBERS: &[&str] = &[
    // Phase 1: Protocol + API
    "protocol",
    "codex-api",
    "codex-client",
    "codex-experimental-api-macros",
    // Phase 1 deps
    "utils/string",
    "utils/stream-parser",
    "utils/absolute-path",
    "utils/image",
    "utils/cache",
    "git-utils",
    "utils/home-dir",
    // "utils/oss",  // depends on codex_lmstudio/codex_ollama (stripped)
    "utils/json-to-toml",
    "utils/fuzzy-match",
    "utils/elapsed",
    "utils/sandbox-summary",
    "utils/approval-presets",
    "utils/cli",
    "utils/cargo-bin",
    // "utils/rustls-provider",  // stripped: depends on rustls (native TLS not needed in WASM)
    "utils/pty",
    "utils/output-truncation",
    "utils/path-utils",
    "utils/plugins",
    "utils/template",
    "terminal-detection",
    // Phase 2: Agent loop
    "execpolicy",
    "execpolicy-legacy",
    "apply-patch",
    "shell-command",
    "shell-escalation",
    "config",
    "core",
    "state",
    "skills",
    "hooks",
    "secrets",
    "connectors",
    "features",
    "app-server-protocol",
    "keyring-store",
    "core-skills",
    "instructions",
    "plugin",
    "rollout",
    "sandboxing",
    "tools",
    // Phase 2 deps
    "ansi-escape",
    "async-utils",
    "arg0",
    "rmcp-client",
    // "mcp-server",  // deeply tied to native CLI (codex-arg0, tracing layers) — not needed in WASM
    "file-search",
    "network-proxy",
    "exec-server",
    // Phase 3: App server (general-purpose agent backend)
    "app-server",
    "app-server-client",
    // Phase 4: TUI deps
    // "feedback",  // stripped — uses sentry (native crash reporting). Stubbed via wasi-codex-feedback.
    "utils/sleep-inhibitor",
    // "tui",  // compiled via codex-wasm-tui standalone workspace
    // "cli",
];

/// Target-specific dependency section patterns to strip.
#[allow(dead_code)]
const STRIP_TARGET_SECTIONS: &[&str] = &[
    "target.'cfg(unix)'.dependencies",
    "target.'cfg(windows)'.dependencies",
    "target.'cfg(target_os = \"linux\")'.dependencies",
    "target.'cfg(target_os = \"android\")'.dependencies",
    "target.'cfg(not(target_os = \"android\"))'.dependencies",
    "target.'cfg(not(target_os = \"linux\"))'.dependencies",
];

pub fn transform_workspace(codex_rs: &Path, dry_run: bool) -> Result<()> {
    // Transform workspace root Cargo.toml
    let root_cargo = codex_rs.join("Cargo.toml");
    if root_cargo.exists() {
        transform_workspace_root(&root_cargo, dry_run)?;
    }

    // Parse workspace dependencies AFTER root transform (so shim redirects are applied)
    let workspace_deps = parse_workspace_deps(&root_cargo)?;

    // Transform each crate's Cargo.toml
    for entry in WalkDir::new(codex_rs)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.file_name().map(|f| f == "Cargo.toml").unwrap_or(false) && path != root_cargo {
            transform_crate_cargo(path, codex_rs, dry_run, &workspace_deps)
                .with_context(|| format!("transforming {}", path.display()))?;
        }
    }

    Ok(())
}

/// Parsed workspace dependency definitions from [workspace.dependencies].
type WorkspaceDeps = std::collections::HashMap<String, toml_edit::Item>;

/// Parse [workspace.dependencies] from the workspace root Cargo.toml.
fn parse_workspace_deps(root_cargo: &Path) -> Result<WorkspaceDeps> {
    let content = std::fs::read_to_string(root_cargo)?;
    let doc = content.parse::<DocumentMut>()?;
    let mut deps = WorkspaceDeps::new();

    if let Some(ws_deps) = doc
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(|d| d.as_table_like())
    {
        for (key, value) in ws_deps.iter() {
            deps.insert(key.to_string(), value.clone());
        }
    }

    Ok(deps)
}

fn transform_workspace_root(path: &Path, dry_run: bool) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let mut doc = content.parse::<DocumentMut>()?;

    // 1. Keep edition 2024 — stable Rust 1.85+ supports it, and upstream
    // uses 2024 features (let chains, etc.) extensively.

    // 2. Remove workspace members not in the allowlist
    if let Some(members) = doc
        .get_mut("workspace")
        .and_then(|w| w.get_mut("members"))
        .and_then(|m| m.as_array_mut())
    {
        let mut to_remove = Vec::new();
        for (i, member) in members.iter().enumerate() {
            if let Some(name) = member.as_str() {
                if !KEEP_WORKSPACE_MEMBERS.contains(&name) {
                    to_remove.push(i);
                    println!("  [workspace.members] remove {name}");
                }
            }
        }
        // Remove in reverse order to preserve indices
        for i in to_remove.into_iter().rev() {
            members.remove(i);
        }

        // Add any KEEP members that aren't currently in the workspace
        // (e.g., arg0, feedback, utils/sleep-inhibitor are not upstream workspace members
        // but are needed by the TUI)
        let current_members: Vec<String> = members
            .iter()
            .filter_map(|m| m.as_str().map(String::from))
            .collect();
        for &keep in KEEP_WORKSPACE_MEMBERS {
            if !current_members.iter().any(|m| m == keep) {
                // Check if the directory actually exists in upstream
                let member_dir = path.parent().unwrap().join(keep);
                if member_dir.join("Cargo.toml").exists() {
                    members.push(keep);
                    println!("  [workspace.members] add {keep}");
                }
            }
        }
    }

    // 3. Strip workspace dependencies for stripped crates
    if let Some(deps) = doc
        .get_mut("workspace")
        .and_then(|w| w.get_mut("dependencies"))
        .and_then(|d| d.as_table_like_mut())
    {
        for dep_name in STRIP_DEPS {
            if deps.contains_key(dep_name) {
                deps.remove(dep_name);
                println!("  [workspace.dependencies] strip {dep_name}");
            }
        }

        // Ensure futures has alloc feature (needed for BoxStream in codex-client)
        if let Some(futures_dep) = deps.get_mut("futures") {
            if let Some(table) = futures_dep.as_inline_table_mut() {
                if let Some(features) = table.get_mut("features") {
                    if let Some(arr) = features.as_array_mut() {
                        if !arr.iter().any(|v| v.as_str() == Some("alloc")) {
                            arr.push("alloc");
                            println!("  [workspace.dependencies] futures: add alloc feature");
                        }
                    }
                } else {
                    let mut arr = toml_edit::Array::new();
                    arr.push("alloc");
                    table.insert("features", Value::Array(arr));
                    println!("  [workspace.dependencies] futures: add alloc feature");
                }
            }
        }

        // Redirect shim deps
        for (dep_name, shim_crate) in SHIM_REDIRECTS {
            if deps.contains_key(dep_name) {
                let mut table = toml_edit::InlineTable::new();
                let relative_path = format!("../../codex-wasm/{shim_crate}");
                table.insert("path", Value::from(relative_path.as_str()));
                deps.insert(dep_name, Item::Value(Value::InlineTable(table)));
                println!("  [workspace.dependencies] redirect {dep_name} → {shim_crate}");
            }
        }

        // Redirect submodule deps (tree-sitter forks etc.)
        for (dep_name, relative_path) in SUBMODULE_REDIRECTS {
            if deps.contains_key(dep_name) {
                let mut table = toml_edit::InlineTable::new();
                table.insert("path", Value::from(*relative_path));
                deps.insert(dep_name, Item::Value(Value::InlineTable(table)));
                println!("  [workspace.dependencies] redirect {dep_name} → {relative_path}");
            }
        }
    }

    // 4. Clean up [patch.crates-io] — keep websocket patches, remove shim-redirected ones
    if let Some(patch) = doc.get_mut("patch") {
        if let Some(crates_io) = patch
            .get_mut("crates-io")
            .and_then(|c| c.as_table_like_mut())
        {
            // Remove patches for crates we redirect to shims
            for name in &["crossterm", "ratatui"] {
                if crates_io.contains_key(name) {
                    crates_io.remove(name);
                    println!("  [patch.crates-io] remove {name} (redirected to shim)");
                }
            }
            // Keep tokio-tungstenite and tungstenite patches — needed for websockets

            // Inject patches for transitive dependency unification
            for (dep_name, relative_path) in INJECT_PATCHES {
                let mut table = toml_edit::InlineTable::new();
                table.insert("path", Value::from(*relative_path));
                crates_io.insert(dep_name, Item::Value(Value::InlineTable(table)));
                println!("  [patch.crates-io] inject {dep_name} → {relative_path}");
            }
        }
    }

    if !dry_run {
        std::fs::write(path, doc.to_string())?;
    }
    println!("  ✓ {}", path.display());
    Ok(())
}

fn transform_crate_cargo(
    path: &Path,
    codex_rs: &Path,
    dry_run: bool,
    workspace_deps: &WorkspaceDeps,
) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let mut doc = content.parse::<DocumentMut>()?;
    let mut changed = false;

    // Determine this crate's directory name for per-crate transforms
    let crate_dir_name = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|f| f.to_str())
        .unwrap_or("");

    let crate_dir = path.parent().unwrap();

    // 1. Redirect shim deps in [dependencies]
    for section_name in &["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(deps) = doc
            .get_mut(section_name)
            .and_then(|d| d.as_table_like_mut())
        {
            for (dep_name, shim_crate) in SHIM_REDIRECTS {
                if deps.contains_key(dep_name) {
                    // Compute relative path from this crate to the shim.
                    // codex_rs = runtime/codex-upstream/codex-rs
                    // shims live at runtime/codex-wasm/<shim>/
                    let shim_path = codex_rs
                        .parent() // runtime/codex-upstream/
                        .unwrap()
                        .parent() // runtime/
                        .unwrap()
                        .join("codex-wasm")
                        .join(shim_crate);
                    let relative = pathdiff(crate_dir, &shim_path);
                    let mut table = toml_edit::InlineTable::new();
                    table.insert("path", Value::from(relative.as_str()));
                    deps.insert(dep_name, Item::Value(Value::InlineTable(table)));
                    changed = true;
                }
            }

            // Strip global deps
            for dep_name in STRIP_DEPS {
                if deps.contains_key(dep_name) {
                    deps.remove(dep_name);
                    changed = true;
                }
            }

            // Strip per-crate deps
            for (target_crate, extra_deps) in PER_CRATE_STRIP_DEPS {
                if crate_dir_name == *target_crate {
                    for dep_name in *extra_deps {
                        if deps.contains_key(dep_name) {
                            deps.remove(dep_name);
                            changed = true;
                            println!("  [per-crate] {target_crate}: strip {dep_name}");
                        }
                    }
                }
            }

            // Inject extra deps for specific crates (add deps the TUI needs that
            // aren't in its original Cargo.toml)
            for (target_crate, extra_deps) in INJECT_DEPS {
                if crate_dir_name == *target_crate {
                    for (dep_name, dep_path) in *extra_deps {
                        if !deps.contains_key(dep_name) {
                            let resolved_path = crate_dir.join(dep_path);
                            let relative = pathdiff(crate_dir, &resolved_path);
                            let mut table = toml_edit::InlineTable::new();
                            table.insert("path", Value::from(relative.as_str()));
                            deps.insert(dep_name, Item::Value(Value::InlineTable(table)));
                            changed = true;
                            println!("  [inject] {target_crate}: add {dep_name}");
                        }
                    }
                }
            }

            // Strip rmcp transport features that pull in heavy deps
            if let Some(rmcp_dep) = deps.get_mut("rmcp") {
                changed |= strip_rmcp_features(rmcp_dep);
            }
        }
    }

    // 2b. Clean up [features] that reference stripped deps (e.g. `dep:cpal`, `dep:hound`).
    if let Some(features) = doc.get_mut("features").and_then(|f| f.as_table_like_mut()) {
        // Remove feature entries that reference any stripped dep via `dep:<name>`.
        let stripped_feature_keys: Vec<String> = features
            .iter()
            .filter_map(|(key, val)| {
                if let Some(arr) = val.as_array() {
                    let has_stripped_dep = arr.iter().any(|v| {
                        if let Some(s) = v.as_str() {
                            if let Some(dep_name) = s.strip_prefix("dep:") {
                                return STRIP_DEPS.contains(&dep_name);
                            }
                        }
                        false
                    });
                    if has_stripped_dep {
                        return Some(key.to_string());
                    }
                }
                None
            })
            .collect();
        for key in &stripped_feature_keys {
            features.remove(key);
            changed = true;
            println!("  [features] strip {key} (references stripped dep)");
        }

        // Also strip features from the default list that were removed.
        if let Some(default_val) = features.get_mut("default") {
            if let Some(arr) = default_val.as_array_mut() {
                let orig_len = arr.len();
                arr.retain(|v| {
                    v.as_str()
                        .map(|s| !stripped_feature_keys.contains(&s.to_string()))
                        .unwrap_or(true)
                });
                if arr.len() != orig_len {
                    changed = true;
                    println!("  [features] cleaned default feature list");
                }
            }
        }
    }

    // Also switch syntect from onig (C dep) to fancy-regex (pure Rust) for WASM.
    if let Some(deps) = doc
        .get_mut("dependencies")
        .and_then(|d| d.as_table_like_mut())
    {
        // syntect: default-features = false, features = ["default-fancy"]
        if let Some(syntect_dep) = deps.get_mut("syntect") {
            if let Some(table) = syntect_dep.as_inline_table_mut() {
                table.insert("default-features", Value::from(false));
                let mut arr = toml_edit::Array::new();
                arr.push("default-fancy");
                table.insert("features", Value::Array(arr));
                changed = true;
                println!("  [syntect] switch to fancy-regex backend");
            } else if syntect_dep.is_str() {
                // Simple version string — convert to inline table
                let version = syntect_dep.as_str().unwrap_or("5").to_string();
                let mut table = toml_edit::InlineTable::new();
                table.insert("version", Value::from(version.as_str()));
                table.insert("default-features", Value::from(false));
                let mut arr = toml_edit::Array::new();
                arr.push("default-fancy");
                table.insert("features", Value::Array(arr));
                *syntect_dep = Item::Value(Value::InlineTable(table));
                changed = true;
                println!("  [syntect] switch to fancy-regex backend");
            }
        }
        // two-face: syntect-default-onig → syntect-default-fancy
        if let Some(two_face_dep) = deps.get_mut("two-face") {
            if let Some(table) = two_face_dep.as_inline_table_mut() {
                if let Some(features) = table.get_mut("features") {
                    if let Some(arr) = features.as_array_mut() {
                        let mut replaced = false;
                        for item in arr.iter_mut() {
                            if item.as_str() == Some("syntect-default-onig") {
                                *item = Value::from("syntect-default-fancy");
                                replaced = true;
                            }
                        }
                        if replaced {
                            changed = true;
                            println!("  [two-face] switch to fancy-regex backend");
                        }
                    }
                }
            }
        }
    }

    // 3. Strip ALL target-specific dependency sections.
    // We're targeting wasm32 only — none of these are relevant.
    if doc.contains_key("target") {
        doc.remove("target");
        changed = true;
    }

    // 4. Rescue deps that were under [target] sections but need to stay.
    for (target_crate, dep_name) in RESCUE_TARGET_DEPS {
        if crate_dir_name == *target_crate {
            if let Some(deps) = doc
                .get_mut("dependencies")
                .and_then(|d| d.as_table_like_mut())
            {
                if !deps.contains_key(dep_name) {
                    let mut table = toml_edit::InlineTable::new();
                    table.insert("workspace", Value::from(true));
                    deps.insert(dep_name, Item::Value(Value::InlineTable(table)));
                    changed = true;
                    println!("  [rescue] {target_crate}: re-add {dep_name}");
                }
            }
        }
    }

    // 5. Resolve all remaining `{ workspace = true }` references to concrete definitions.
    // This is critical: it allows crates from codex-upstream to be used as standalone
    // path dependencies from the main workspace, not just within the upstream workspace.
    for section_name in &["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(deps) = doc
            .get_mut(section_name)
            .and_then(|d| d.as_table_like_mut())
        {
            // Collect keys that need resolving (can't mutate while iterating)
            let keys_to_resolve: Vec<String> = deps
                .iter()
                .filter_map(|(key, value)| {
                    if has_workspace_true(value) {
                        Some(key.to_string())
                    } else {
                        None
                    }
                })
                .collect();

            for key in keys_to_resolve {
                if let Some(ws_def) = workspace_deps.get(&key) {
                    let local_item = deps.get(&key).cloned();
                    let resolved =
                        resolve_workspace_dep(ws_def, local_item.as_ref(), codex_rs, crate_dir);
                    deps.insert(&key, resolved);
                    changed = true;
                }
            }
        }
    }

    // 6. Remove workspace-inherited package fields
    // Remove `edition.workspace = true`, `version.workspace = true`, `license.workspace = true`
    // and replace with concrete values
    if let Some(pkg) = doc.get_mut("package").and_then(|p| p.as_table_like_mut()) {
        if is_workspace_inherited(pkg.get("edition")) {
            pkg.insert("edition", Item::Value(Value::from("2024")));
            changed = true;
        }
        if is_workspace_inherited(pkg.get("version")) {
            pkg.insert("version", Item::Value(Value::from("0.0.0")));
            changed = true;
        }
        if is_workspace_inherited(pkg.get("license")) {
            pkg.insert("license", Item::Value(Value::from("Apache-2.0")));
            changed = true;
        }
    }

    // 7. Remove [lints] workspace = true
    if let Some(lints) = doc.get("lints") {
        if is_workspace_inherited(Some(lints)) {
            doc.remove("lints");
            changed = true;
        }
    }

    if changed {
        if !dry_run {
            std::fs::write(path, doc.to_string())?;
        }
        println!("  ✓ {}", path.display());
    }

    Ok(())
}

/// Check if an item has `workspace = true`.
fn has_workspace_true(item: &Item) -> bool {
    if let Some(table) = item.as_inline_table() {
        return table.get("workspace").and_then(|v| v.as_bool()) == Some(true);
    }
    if let Some(table) = item.as_table_like() {
        return table.get("workspace").and_then(|v| v.as_bool()) == Some(true);
    }
    false
}

/// Check if a package field has `{ workspace = true }`.
fn is_workspace_inherited(item: Option<&Item>) -> bool {
    item.map(|i| has_workspace_true(i)).unwrap_or(false)
}

/// Resolve a workspace dependency to a concrete definition.
///
/// Takes the workspace-level definition and merges in any local overrides
/// (e.g., extra features, optional = true). Adjusts relative paths.
fn resolve_workspace_dep(
    ws_def: &Item,
    local_item: Option<&Item>,
    codex_rs: &Path,
    crate_dir: &Path,
) -> Item {
    // Extract local overrides (features, optional, default-features)
    let local_features: Option<Vec<String>> =
        local_item.and_then(|item| get_features_from_item(item));
    let local_optional: Option<bool> = local_item.and_then(|item| get_bool_field(item, "optional"));
    let local_default_features: Option<bool> =
        local_item.and_then(|item| get_bool_field(item, "default-features"));

    // Build resolved item from workspace definition
    let mut table = toml_edit::InlineTable::new();

    // Simple version string: `anyhow = "1"`
    if let Some(version_str) = ws_def.as_str() {
        table.insert("version", Value::from(version_str));
    }
    // Table form: `foo = { version = "1", features = [...] }`
    else if let Some(ws_table) = ws_def.as_inline_table() {
        for (k, v) in ws_table.iter() {
            if k == "workspace" {
                continue;
            }
            if k == "path" {
                // Adjust relative path: workspace paths are relative to codex_rs
                if let Some(p) = v.as_str() {
                    let abs_path = codex_rs.join(p);
                    let relative = pathdiff(crate_dir, &abs_path);
                    table.insert("path", Value::from(relative.as_str()));
                    continue;
                }
            }
            table.insert(k, v.clone());
        }
    } else if let Some(ws_table) = ws_def.as_table_like() {
        for (k, v) in ws_table.iter() {
            if k == "workspace" {
                continue;
            }
            if k == "path" {
                if let Some(p) = v.as_str() {
                    let abs_path = codex_rs.join(p);
                    let relative = pathdiff(crate_dir, &abs_path);
                    table.insert("path", Value::from(relative.as_str()));
                    continue;
                }
            }
            // For table items, try to get the value
            if let Some(val) = v.as_value() {
                table.insert(k, val.clone());
            }
        }
    }

    // Merge local overrides
    if let Some(opt) = local_optional {
        table.insert("optional", Value::from(opt));
    }
    if let Some(df) = local_default_features {
        table.insert("default-features", Value::from(df));
    }

    // Merge features: workspace features + local features (deduplicated)
    if local_features.is_some() {
        let mut all_features: Vec<String> = Vec::new();

        // Start with workspace features
        if let Some(ws_feats) = table.get("features").and_then(|v| v.as_array()) {
            for f in ws_feats.iter() {
                if let Some(s) = f.as_str() {
                    all_features.push(s.to_string());
                }
            }
        }

        // Add local features
        if let Some(local_feats) = local_features {
            for f in local_feats {
                if !all_features.contains(&f) {
                    all_features.push(f);
                }
            }
        }

        let mut arr = toml_edit::Array::new();
        for f in &all_features {
            arr.push(f.as_str());
        }
        table.insert("features", Value::Array(arr));
    }

    Item::Value(Value::InlineTable(table))
}

fn get_features_from_item(item: &Item) -> Option<Vec<String>> {
    let get_from_table = |t: &dyn toml_edit::TableLike| -> Option<Vec<String>> {
        t.get("features").and_then(|f| f.as_array()).map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
    };

    if let Some(t) = item.as_inline_table() {
        return get_from_table(t);
    }
    if let Some(t) = item.as_table_like() {
        return get_from_table(t);
    }
    None
}

fn get_bool_field(item: &Item, field: &str) -> Option<bool> {
    if let Some(t) = item.as_inline_table() {
        return t.get(field).and_then(|v| v.as_bool());
    }
    if let Some(t) = item.as_table_like() {
        return t.get(field).and_then(|v| v.as_bool());
    }
    None
}

/// Strip problematic rmcp features from a dependency item.
fn strip_rmcp_features(dep: &mut Item) -> bool {
    let mut changed = false;

    // Handle inline table: rmcp = { workspace = true, features = [...] }
    if let Some(table) = dep.as_inline_table_mut() {
        if let Some(features) = table.get_mut("features") {
            if let Some(arr) = features.as_array_mut() {
                let mut to_remove = Vec::new();
                for (i, val) in arr.iter().enumerate() {
                    if let Some(s) = val.as_str() {
                        if STRIP_RMCP_FEATURES.contains(&s) {
                            to_remove.push(i);
                        }
                    }
                }
                for i in to_remove.into_iter().rev() {
                    arr.remove(i);
                    changed = true;
                }
                if changed {
                    println!("  [rmcp] stripped transport features");
                }
            }
        }
    }

    // Handle regular table
    if let Some(table) = dep.as_table_like_mut() {
        if let Some(features) = table.get_mut("features") {
            if let Some(arr) = features.as_array_mut() {
                let mut to_remove = Vec::new();
                for (i, val) in arr.iter().enumerate() {
                    if let Some(s) = val.as_str() {
                        if STRIP_RMCP_FEATURES.contains(&s) {
                            to_remove.push(i);
                        }
                    }
                }
                for i in to_remove.into_iter().rev() {
                    arr.remove(i);
                    changed = true;
                }
                if changed {
                    println!("  [rmcp] stripped transport features");
                }
            }
        }
    }

    changed
}

/// Simple relative path computation between two directories.
fn pathdiff(from: &Path, to: &Path) -> String {
    // Simple implementation: count ".." needed and append target components
    let from = from.canonicalize().unwrap_or_else(|_| from.to_path_buf());
    let to = to.canonicalize().unwrap_or_else(|_| to.to_path_buf());

    let from_components: Vec<_> = from.components().collect();
    let to_components: Vec<_> = to.components().collect();

    // Find common prefix length
    let common = from_components
        .iter()
        .zip(to_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let ups = from_components.len() - common;
    let mut result = String::new();
    for _ in 0..ups {
        if !result.is_empty() {
            result.push('/');
        }
        result.push_str("..");
    }
    for component in &to_components[common..] {
        if !result.is_empty() {
            result.push('/');
        }
        result.push_str(&component.as_os_str().to_string_lossy());
    }

    if result.is_empty() {
        ".".to_string()
    } else {
        result
    }
}
