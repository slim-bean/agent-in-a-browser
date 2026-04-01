# Handoff: Complete wasm32-wasip2 Build for App-Server Architecture

## Branch

```sh
git checkout rebase-app-server-architecture
```

The codex-upstream submodule points to `wasm32-wasip2` branch on `github.com/tjfontaine/codex.git` (commit `5cfbd2746`).

## What Was Done

We rebased codex-upstream by 256 commits (to `868ac158d`) and made a strategic architecture change: instead of stubbing `codex-app-server` and `codex-app-server-client` with bail-returning types, we now **keep the real crates** in the WASM build. Only platform-specific edges are stubbed (axum websocket, JWT auth, stdio transport, file watcher, command exec, tracing).

This matters because upstream removed the legacy direct-to-Codex TUI path. All interaction now goes through:
```
TUI → AppServerSession → AppServerClient → codex-app-server::in_process → MessageProcessor → codex-core
```

The `codex-app-server` and `codex-app-server-client` both compile for `wasm32-wasip2`. The TUI has 2 remaining compile errors.

## What Remains

### Immediate: Fix 2 TUI compile errors

The `string_replace` transforms for these are already written in `syn_transforms.rs` but haven't been verified in a clean codemod + build cycle. Run:

```sh
# Reset upstream, re-run codemod, build for wasm32
cd runtime/codex-upstream && git checkout upstream/main -- .
cd ../..
cargo run -p codex-codemod -- runtime/codex-upstream/

# Build for wasm32-wasip2
export WASI_SDK_PATH=/home/tjfontaine/.local/wasi-sdk-32.0-x86_64-linux
export CC_wasm32_wasip2="${WASI_SDK_PATH}/bin/clang"
export CFLAGS_wasm32_wasip2="--sysroot=${WASI_SDK_PATH}/share/wasi-sysroot -D_WASI_EMULATED_PROCESS_CLOCKS -D_WASI_EMULATED_SIGNAL -D_WASI_EMULATED_MMAN"
cargo component check --manifest-path runtime/codex-wasm/codex-wasm-tui/Cargo.toml --target wasm32-wasip2
```

The 2 known errors (transforms already added, should resolve):
1. **`tui/src/app.rs:3551`** — `None` needs type annotation → `None::<String>` (string_replace in syn_transforms.rs)
2. **`tui/src/chatwidget.rs:2131`** — `feedback_diagnostics()` returns value but callee expects `&` → prepend `&` (string_replace in syn_transforms.rs)

If more errors appear, follow the iterative pattern established in this work.

### Iterative Build-Fix Pattern

When you hit a compile error for the wasm32-wasip2 target, the fix is usually one of:

1. **Missing type/function from a stripped crate** → Add a `string_replace` in `syn_transforms.rs` to stub the import inline, OR update an existing replacement file in `replacements/`
2. **Missing type in a shim crate** (wasi-sqlx, wasi-codex-otel, wasi-tokio, etc.) → Add the type/method to the shim
3. **Unused import/variable warning treated as error** → Add the file to `PREPEND_TEXT` in `syn_transforms.rs` with appropriate `#![allow(...)]`
4. **New upstream crate needed** → Add to `KEEP_WORKSPACE_MEMBERS` in `cargo_toml.rs`
5. **Whole module needs stubbing** → Create a replacement file in `replacements/` and register it in `transforms/stubs.rs`

### After Build Succeeds: Verify Host Target Still Works

```sh
# Host target (must also pass)
cd runtime
cargo check --workspace --all-targets --exclude wasmtime-runner
cargo fmt --all --check
```

### After Both Targets Pass: Commit Codex-Upstream and Update Submodule

```sh
cd runtime/codex-upstream
git add -A
git commit -m "feat: apply wasip2 codemod — full wasm32-wasip2 build"
git push origin HEAD:wasm32-wasip2 --force-with-lease
cd ../..
git add runtime/codex-upstream
# Commit parent repo changes
```

### Longer Term: Build the Full WASM Binary

Once `cargo component check` passes, the next step is `cargo component build` (the actual WASM binary). This may surface linker-level issues not caught by check. Then:

```sh
# Full build pipeline
pnpm build:wasm    # moon run runtime:build-wasm
pnpm build         # Full monorepo build
pnpm test          # Unit tests
pnpm test:e2e      # Playwright E2E
```

## Architecture Context

The plan file at `.claude/plans/nifty-tinkering-puddle.md` has the full strategic context. Key points:

- The app-server is upstream's general-purpose agent backend (JSON-RPC 2.0, transport-agnostic)
- We keep the `in_process` path working — `MessageProcessor` is platform-neutral
- Only 6 app-server modules are stubbed (transport/websocket, transport/auth, transport/stdio, fs_watch, command_exec, app_server_tracing) plus the remote client
- `codex_message_processor.rs` (9k lines) has 2 inline stubs for stripped crates (`codex_backend_client`, `codex_cloud_requirements`)
- The `merge_connectors_with_accessible` rename is handled carefully: AST rename only fires in TUI chatwidget files, and a 3-arg version is injected via `string_replacements.rs` for app-server call sites

## Key Files

| File | Purpose |
|------|---------|
| `runtime/codex-wasm/codex-codemod/src/cargo_toml.rs` | `KEEP_WORKSPACE_MEMBERS`, `STRIP_DEPS`, `PER_CRATE_STRIP_DEPS`, `INJECT_DEPS` |
| `runtime/codex-wasm/codex-codemod/src/syn_transforms.rs` | `PREPEND_TEXT`, `string_replace`, `replace_in_file`, AST rename rules |
| `runtime/codex-wasm/codex-codemod/src/transforms/stubs.rs` | `ReplaceFile` entries (maps path suffixes to replacement content) |
| `runtime/codex-wasm/codex-codemod/src/transforms/string_replacements.rs` | `ReplaceFirst` for injecting connector functions |
| `runtime/codex-wasm/codex-codemod/replacements/app-server/` | 6 stub files for platform-specific app-server modules |
| `runtime/codex-wasm/codex-codemod/replacements/app-server-client/src/remote.rs` | Remote client stub |
| `runtime/codex-wasm/wasi-sqlx/src/{lib,sqlite}.rs` | SQLite shim (updated: SqliteAutoVacuum, &&str IntoQueryParam) |
| `runtime/codex-wasm/wasi-codex-otel/src/lib.rs` | OpenTelemetry shim (updated: TelemetryAuthMode::Chatgpt, sanitize_metric_tag_value, span_w3c_trace_context) |

## Build Commands Quick Reference

```sh
# Codemod only
cargo run -p codex-codemod -- runtime/codex-upstream/

# Host check (fast, catches most issues)
cd runtime && cargo check --workspace --all-targets --exclude wasmtime-runner

# WASM check (slower, catches target-specific issues)
cargo component check --manifest-path runtime/codex-wasm/codex-wasm-tui/Cargo.toml --target wasm32-wasip2

# Format check
cd runtime && cargo fmt --all --check

# Full CI validation
moon run runtime:fmt-check runtime:check runtime:build-wasm runtime:verify-wasm runtime:check-native runtime:test frontend:build frontend:copy-externals frontend:test
```
