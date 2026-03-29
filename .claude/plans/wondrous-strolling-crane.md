# Plan: Fork Codex CLI for wasip2

## Context

Replace the current rig-core agent loop + custom ratatui TUI with a fork of OpenAI's Codex CLI (`codex-rs/`), adapted to run as a wasip2 WASM component. The critical constraint: **keep the fork easy to sync with upstream** by building automated codemod tools that can be reapplied whenever upstream changes are pulled.

Codex `core/` is 361 files (~210k LOC), `tui/` is 137 files (~157k LOC). Both use tokio heavily. The strategy minimizes diff from upstream by using **shim crates** (fake `tokio`, fake `reqwest`, etc.) rather than rewriting source — so upstream code stays nearly unchanged.

**Decisions made:**
- **Codemod engine**: `syn` + `quote` for AST transforms, `toml_edit` for Cargo.toml
- **Async strategy**: wasi-tokio shim crate (smallest upstream diff)
- **TUI scope**: Port both agent core AND Codex TUI

---

## 1. Git Strategy: Subtree + Codemod Script

```
runtime/
  codex-upstream/           # git subtree from openai/codex codex-rs/ dir
    protocol/
    core/
    tui/
    codex-api/
    ...
  codex-wasm/               # Our code (never auto-modified by codemod)
    platform-traits/        # Platform abstraction trait crate
    wasi-impl/              # WASI implementations of platform traits
    wasi-tokio/             # tokio-compatible API shim for wasip2
    wasi-reqwest/           # reqwest-compatible API shim for wasip2
    wasi-crossterm/         # crossterm-compatible shim for browser terminal
    codex-wasm-agent/       # WASM component entry point (cdylib)
    codex-codemod/          # The codemod tool binary
  crates/                   # Existing crates (keep during migration)
```

**Sync workflow:**
```bash
# 1. Pull upstream changes into subtree
git subtree pull --prefix=runtime/codex-upstream \
  https://github.com/openai/codex.git main --squash

# 2. Re-apply codemods (idempotent)
cargo run -p codex-codemod -- runtime/codex-upstream/

# 3. Verify
cargo component check -p codex-wasm-agent --target wasm32-wasip2

# 4. Commit
git add runtime/codex-upstream/ && git commit -m "sync: reapply WASM codemods"
```

Codemods are applied **in-tree** so `cargo check` works on the repo as-is.

---

## 2. Shim Crate Strategy (Key Design Decision)

Rather than rewriting every `tokio::spawn`, `reqwest::Client`, and `crossterm::event` call site (massive diff, impossible to sync), we create **drop-in shim crates** that provide the same API surface but backed by WASI primitives.

The codemod only modifies **Cargo.toml** files — not Rust source. This is the smallest possible diff from upstream.

### `wasi-tokio` — tokio API shim for wasip2

Provides the subset of tokio APIs that Codex actually uses:

```rust
// runtime/codex-wasm/wasi-tokio/src/lib.rs
// Re-exports that match tokio's module structure

pub mod sync {
    pub use std::sync::Mutex;          // WASM is single-threaded
    pub use std::sync::RwLock;
    pub mod mpsc { /* async_channel wrapper */ }
    pub mod oneshot { /* single-value channel */ }
    pub mod broadcast { /* simple bus */ }
}

pub mod process {
    // Command/Child backed by WIT shell interface
    pub struct Command { ... }
    pub struct Child { ... }
}

pub mod fs {
    // Async fs ops backed by wasi:filesystem
    pub async fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> { ... }
    pub async fn write(path: impl AsRef<Path>, data: impl AsRef<[u8]>) -> io::Result<()> { ... }
}

pub mod time {
    pub async fn sleep(duration: Duration) { /* wasi:clocks */ }
    pub struct Interval { ... }
}

pub mod io {
    pub trait AsyncRead { ... }  // Minimal impl
    pub trait AsyncWrite { ... }
}

pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where F: Future + Send + 'static {
    // Single-threaded: poll to completion inline (via JSPI suspend)
}

// select! macro replacement
macro_rules! select { ... }
```

### `wasi-reqwest` — reqwest API shim for wasip2

```rust
// Wraps wasi:http/outgoing-handler with reqwest-compatible API
pub struct Client { ... }
pub struct RequestBuilder { ... }
pub struct Response { ... }

impl Client {
    pub fn new() -> Self { ... }
    pub fn get(&self, url: &str) -> RequestBuilder { ... }
    pub fn post(&self, url: &str) -> RequestBuilder { ... }
}
```

### `wasi-crossterm` — crossterm shim for browser terminal

```rust
// Maps crossterm terminal events to browser terminal (xterm.js)
// Input events come from WIT interface (imported from JS)
pub mod event {
    pub enum Event { Key(KeyEvent), Mouse(MouseEvent), Resize(u16, u16) }
    pub fn poll(timeout: Duration) -> Result<bool> { ... }
    pub fn read() -> Result<Event> { ... }
}

pub mod terminal {
    pub fn enable_raw_mode() -> Result<()> { Ok(()) } // no-op in browser
    pub fn size() -> Result<(u16, u16)> { /* query from WIT */ }
}

pub mod cursor { ... }
pub mod style { ... }
```

### What the codemod does to Cargo.toml

For each crate in codex-upstream that depends on `tokio`:

```toml
# BEFORE (upstream)
[dependencies]
tokio = { workspace = true, features = ["macros", "rt-multi-thread", "process"] }

# AFTER (codemod applied)
[dependencies]
tokio = { path = "../../codex-wasm/wasi-tokio" }
```

Same for `reqwest` → `wasi-reqwest`, `crossterm` → `wasi-crossterm`.

**This is the entire source-level change.** Upstream `.rs` files remain untouched for these transforms.

---

## 3. Codemod Tool (`codex-codemod`)

Built with `syn` + `quote` + `toml_edit`. Handles transforms that can't be done via shim crates alone.

### Cargo.toml Transforms (toml_edit)
1. **Edition**: `2024` → `2021`
2. **Dependency redirection**: `tokio` → `wasi-tokio`, `reqwest` → `wasi-reqwest`, `crossterm` → `wasi-crossterm`
3. **Strip deps**: `keyring`, `landlock`, `seccompiler`, `libc`, `openssl-sys`, `portable-pty`, `arboard`, `webbrowser`, `cpal`, `hound`, `windows-sys`
4. **Strip platform sections**: `[target.'cfg(unix)'.dependencies]`, `[target.'cfg(windows)'.dependencies]`, `[target.'cfg(target_os = "linux")'.dependencies]`
5. **Add platform crate**: `codex-platform = { path = "../../codex-wasm/platform-traits" }`
6. **Remove workspace members** for stripped crates
7. **Remove `[patch.crates-io]`** entries for forked crates we don't need

### AST Transforms (syn, only where shims aren't sufficient)
1. **`#[cfg(unix)]` / `#[cfg(windows)]` blocks** → strip or replace with `#[cfg(target_arch = "wasm32")]`
2. **`libc::*` calls** → remove (in sandbox/signal code)
3. **`std::process::Command`** (non-tokio) → `codex_platform::sync_command()`
4. **Platform-specific modules** → replace with stub files
5. **`unsafe` blocks** referencing platform APIs → remove
6. **Feature gate additions**: wrap stripped code in `#[cfg(not(target_arch = "wasm32"))]` rather than deleting, so the code still compiles for native targets

### Module Stripping
Replace these with empty stub modules:
- `seatbelt.rs`, `landlock.rs`, `windows_sandbox.rs`
- Platform sandbox implementations in `sandboxing/`
- Voice input (`voice.rs`, cpal/hound)
- Clipboard (`arboard`)
- Browser opening (`webbrowser`)
- OS-specific signal handling

### Validation Pass
- `cargo fmt` on all modified files
- `cargo component check --target wasm32-wasip2` on workspace
- Diff report against previous codemod output

---

## 4. Platform Traits (`codex-platform`)

For the few cases where upstream code genuinely can't work with just shim crates (e.g., sandbox policy decisions, process spawning internals):

```rust
pub trait Platform: Send + Sync + 'static {
    fn spawn_command(&self, cmd: &[String], cwd: &Path,
                     env: &HashMap<String, String>,
                     timeout: Option<Duration>) -> Result<ExecOutput>;
    fn sandbox_type(&self) -> SandboxType; // Always returns Wasm
}

pub struct ExecOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
}
```

Kept minimal — most abstraction is handled by the shim crates.

---

## 5. Crate Selection & Phasing

### Phase 0: Infrastructure (foundation)
- [ ] `git subtree add` for codex-rs/
- [ ] Create `wasi-tokio` shim crate (subset: spawn, sync, fs, time, process, io)
- [ ] Create `wasi-reqwest` shim crate (Client, RequestBuilder, Response, SSE streaming)
- [ ] Create `codex-codemod` binary with Cargo.toml transforms
- [ ] Create `codex-platform` trait crate
- [ ] Create `codex-wasm-agent` component crate with WIT world

### Phase 1: Protocol + API (LLM streaming)
Crates: `protocol`, `codex-api`, `codex-client`, `utils/string`, `utils/stream-parser`
- `protocol` — pure data types, needs: edition downgrade, strip `ts-rs`/`icu_*`/`sys-locale`
- `codex-client` — has `HttpTransport` trait. Codemod: redirect `reqwest` → `wasi-reqwest`
- `codex-api` — API types + SSE parsing. Codemod: redirect `tokio` → `wasi-tokio`

**Milestone**: LLM API call + streamed response from wasip2.

### Phase 2: Agent Loop (conversation + tools)
Crates: `execpolicy`, `apply-patch`, `shell-command`, `config`, `core` (subset)
- `core` — redirect `tokio`/`reqwest`, strip sandbox modules, stub platform calls
- Start with: codex.rs, codex_delegate.rs, tools/, stream_events_utils.rs, compact/
- Strip: analytics, realtime_conversation, plugins, js_repl

**Milestone**: Multi-turn agent loop with tool calling in wasip2.

### Phase 3: TUI
Crates: `tui`, `ansi-escape`, `utils/fuzzy-match`, `utils/elapsed`, `utils/sandbox-summary`
- `tui` — redirect `crossterm` → `wasi-crossterm`, redirect `tokio` → `wasi-tokio`
- Strip: voice input, clipboard, browser opening, libc signals
- `wasi-crossterm` delivers terminal events from browser xterm.js via WIT

**Milestone**: Full Codex TUI rendering in browser via WASM.

### Phase 4: Full Features
- `file-search` — redirect `tokio`, stub `ignore` crate with WASI fs walking
- `rmcp-client` — MCP client, redirect `reqwest`/`tokio`
- `skills` — compile-time embedded, cache via WASI fs
- `state` — session persistence via WASI fs (maps to IndexedDB/OPFS)

### Crates to STRIP (never port):
`exec`, `exec-server`, `linux-sandbox`, `windows-sandbox-rs`, `process-hardening`, `network-proxy`, `otel`, `login`, `keyring-store`, `lmstudio`, `ollama`, `app-server`, `app-server-*`, `chatgpt`, `feedback`, `cloud-tasks*`, `backend-client`, `codex-backend-openapi-models`, `responses-api-proxy`, `stdio-to-uds`, `debug-client`, `v8-poc`, `terminal-detection`, `tui_app_server`

---

## 6. WIT World & Pipeline Integration

### WIT Interface
`codex-wasm-agent` exports the **same WIT interface** as existing `headless-agent`:
- `create`, `send-message`, `poll`, `cancel`, `plan`, `execute`, `get-history`, `list-providers`, `list-models`, `fetch-models`
- Same `agent-event` variant type

Drop-in replacement — zero frontend changes.

For the TUI variant, extend the world with terminal I/O:
```wit
// Additional imports for TUI mode
import terminal:io/input { poll-event, read-event }
import terminal:io/output { write-buffer, flush, size }
```

### Build Pipeline
```yaml
# runtime/moon.yml
build-codex-wasm:
  script: |
    cargo run -p codex-codemod -- . &&
    cargo component build --release --target wasm32-wasip2 -p codex-wasm-agent &&
    cargo fmt --all
  deps:
    - "~:wit-deps"
```

Register in `scripts/transpile.mjs` alongside existing modules.

### Migration Path
1. New agent coexists with rig-core agent via build flag
2. Frontend loads whichever WASM module is built
3. Remove rig-core agent + `web-agent-tui` + `web-headless-agent` at feature parity

---

## 7. Shim Coverage Analysis

What the wasi-tokio shim needs to cover (based on Codex usage):

| tokio API | Usage count (core+tui) | Shim strategy |
|-----------|----------------------|---------------|
| `tokio::spawn` | ~20 files | Inline poll via JSPI |
| `tokio::sync::Mutex` | ~15 files | `std::sync::Mutex` |
| `tokio::sync::RwLock` | ~5 files | `std::sync::RwLock` |
| `tokio::sync::mpsc` | ~10 files | `async_channel` wrapper |
| `tokio::sync::broadcast` | ~3 files | Simple Vec<Sender> bus |
| `tokio::sync::oneshot` | ~5 files | Single-value channel |
| `tokio::fs::*` | ~20 files | `wasi:filesystem` |
| `tokio::process::Command/Child` | ~9 files | WIT shell interface |
| `tokio::time::sleep/Instant` | ~10 files | `wasi:clocks` |
| `tokio::io::AsyncRead/Write` | ~5 files | Minimal trait impl |
| `tokio::select!` | ~8 files | Polling macro |
| `tokio::signal` | ~2 files | No-op (no signals in WASM) |
| `#[tokio::test]` | many | `#[test]` + `block_on` |
| `#[tokio::main]` | 2 (bin entries) | Not needed (lib only) |

---

## 8. Verification

1. `cargo run -p codex-codemod -- runtime/codex-upstream/` — codemod runs clean
2. `cargo component check -p codex-wasm-agent --target wasm32-wasip2` — compiles
3. `moon run runtime:build-codex-wasm` — produces `.wasm` artifact
4. JCO transpile succeeds → JS/ESM module
5. Unit: create agent, send message, receive streamed response (mock HTTP in wasi-reqwest)
6. E2E: load in browser SharedWorker, make real LLM call, verify streaming

---

## 9. Key Files

| File | Role |
|------|------|
| `runtime/codex-upstream/core/src/codex.rs` | Agent loop (primary target) |
| `runtime/codex-upstream/core/src/exec.rs` | Process execution → shim to WIT |
| `runtime/codex-upstream/core/src/spawn.rs` | Child spawning → wasi-tokio::process |
| `runtime/codex-upstream/core/src/client.rs` | LLM API → wasi-reqwest |
| `runtime/codex-upstream/tui/src/` | TUI → wasi-crossterm |
| `runtime/codex-upstream/protocol/` | Pure data types (minimal changes) |
| `runtime/codex-wasm/wasi-tokio/src/lib.rs` | tokio API shim (our code) |
| `runtime/codex-wasm/wasi-reqwest/src/lib.rs` | reqwest API shim (our code) |
| `runtime/codex-wasm/wasi-crossterm/src/lib.rs` | crossterm API shim (our code) |
| `runtime/codex-wasm/codex-codemod/src/main.rs` | Codemod tool (our code) |
| `runtime/codex-wasm/codex-wasm-agent/src/lib.rs` | WASM entry point (our code) |
| `runtime/crates/web-headless-agent/wit/world.wit` | WIT interface to match |
| `scripts/transpile.mjs` | JCO config — register new module |
