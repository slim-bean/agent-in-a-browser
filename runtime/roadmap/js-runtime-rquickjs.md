# Implementation Plan: rquickjs-based JavaScript Runtime for Codex Code-Mode (WASM)

## Status: Ready to Implement
## Date: 2026-03-28 (updated)

---

## 1. Problem Statement

The upstream Codex `code-mode` crate provides an in-process JavaScript execution environment for the LLM agent's `exec`/`wait` tools. It runs JS code in a V8 isolate on a dedicated thread, supports async tool calls (returning Promises the V8 runtime resolves later), persistent `store`/`load` values across cells, `text`/`image`/`notify` output helpers, yield control, and execution termination.

In the WASM build (`wasm32-wasip2`), V8 cannot compile. The codemod currently replaces the entire `code-mode/src/runtime/mod.rs` with a stub that returns `Err("v8 runtime not available in WASM")` from `spawn_runtime()`. This means the `exec` and `wait` tools are non-functional in the browser TUI.

This project replaces the V8 backend with rquickjs (QuickJS) for the WASM build, restoring full `exec`/`wait` functionality.

---

## 2. Current Codebase State

### 2.1 The Stub (what we are replacing)

**File:** `runtime/codex-wasm/codex-codemod/src/ast_transforms.rs`, `REPLACE_FILES` entry for `code-mode/src/runtime/mod.rs`

The stub provides all the type definitions that `service.rs` depends on but returns an error from `spawn_runtime()`:

- **Types preserved:** `ExecuteRequest`, `WaitRequest`, `RuntimeResponse`, `TurnMessage`, `RuntimeCommand`, `RuntimeEvent`, `RuntimeHandle`
- **Constants preserved:** `DEFAULT_EXEC_YIELD_TIME_MS`, `DEFAULT_WAIT_YIELD_TIME_MS`, `DEFAULT_MAX_OUTPUT_TOKENS_PER_EXEC_CALL`
- **Stubbed function:** `spawn_runtime()` returns `Err("v8 runtime not available in WASM")`

### 2.2 The tsx-engine (proven rquickjs-on-WASM pattern)

**Location:** `runtime/crates/tsx-engine/`

Already compiles and runs rquickjs on `wasm32-wasip2` in production. Key patterns:

- **rquickjs rev:** `1fe498e7` with features `futures`, `loader`, `classes`, `macro`
- **Runtime setup:** `AsyncRuntime::new()` + `AsyncContext::full(&runtime)` via `futures_lite::future::block_on()`
- **Limits:** `set_memory_limit`, `set_max_stack_size`, `set_gc_threshold`, `set_interrupt_handler`
- **Promise rejection tracking:** `set_host_promise_rejection_tracker`
- **Module evaluation:** `ctx.eval_with_options()` with `EvalOptions` for filename
- **Module loading:** `runtime.set_loader(HybridResolver, HybridLoader)` for imports
- **Global injection:** `js_modules::install_all(&ctx)` installs console, fs, Buffer, etc.
- **Event loop:** `runtime.idle()` drives all pending promises to completion

### 2.3 The Codemod Infrastructure

- **`ast_transforms.rs`:** `REPLACE_FILES` and `REPLACE_FILES_LARGE` swap entire source files
- **`cargo_toml.rs`:** `STRIP_DEPS` removes `v8`; `INJECT_DEPS` adds path-based deps per crate; `SHIM_REDIRECTS` redirects via `[patch.crates-io]`
- **`syn_transforms.rs`:** Rewrites `v8::IsolateHandle` to `crate::runtime::RuntimeHandle` in `service.rs`

### 2.4 The V8 API Surface We Must Replace

From `code-mode/src/runtime/` (upstream):

| Global | Callback | Purpose |
|--------|----------|---------|
| `tools.<name>(input)` | `tool_callback` | Async tool call, returns Promise |
| `ALL_TOOLS` | (array construction) | `[{name, description}]` metadata |
| `text(value)` | `text_callback` | Append text content item |
| `image(url_or_obj)` | `image_callback` | Append image content item |
| `store(key, value)` | `store_callback` | Persist JSON value |
| `load(key)` | `load_callback` | Retrieve stored value |
| `notify(text)` | `notify_callback` | Inject immediate notification |
| `yield_control()` | `yield_control_callback` | Yield output to model |
| `exit()` | `exit_callback` | Terminate script immediately |
| `console` | (deleted) | Explicitly removed from globalThis |

The `service.rs` layer calls `spawn_runtime(request, event_tx)` and gets back `(std::sync::mpsc::Sender<RuntimeCommand>, RuntimeHandle)`. It sends `RuntimeCommand::ToolResponse`, `RuntimeCommand::ToolError`, and `RuntimeCommand::Terminate` through the channel. The runtime sends `RuntimeEvent` variants through the `event_tx`.

---

## 3. Key Design Decision: Reuse tsx-engine or Separate Crate?

### 3.1 Why Not Reuse tsx-engine Directly

tsx-engine is a standalone WASM component (`crate-type = ["cdylib"]`) with its own WIT interface. Code-mode needs to be a library linked into the TUI binary, not a separate component. The APIs are fundamentally different:

- tsx-engine: synchronous eval, returns string output
- code-mode: long-running runtime with bidirectional channel communication, async tool calls via promises, multi-cell lifecycle

### 3.2 Recommendation: New Crate That Shares Patterns

Create `runtime/codex-wasm/wasi-code-runtime/` as a library crate. It:

- Copies the rquickjs setup pattern from tsx-engine (runtime creation, limits, interrupt handler)
- Does NOT need tsx-engine's module loader, Node.js shims, SWC, or filesystem access (code-mode explicitly blocks all imports)
- Implements the code-mode-specific globals (`tools`, `text`, `image`, `store`, `load`, `notify`, `yield_control`, `exit`)
- Exports a single function matching the stub's `spawn_runtime` signature

The crate is small and focused: ~300-500 lines total.

---

## 4. Minimal Viable Implementation

### 4.1 Which Globals Are Actually Used?

Based on the exec tool description sent to the LLM and the upstream implementation:

**Must-have (core functionality):**
- `tools.<name>(input)` — the entire point of code-mode is orchestrating tool calls
- `text(value)` — primary output mechanism
- `store(key, value)` / `load(key)` — cross-cell persistence
- `exit()` — early termination

**Should-have (commonly used):**
- `ALL_TOOLS` — LLM uses this to discover available tools
- `notify(text)` — streaming intermediate output
- `yield_control()` — explicit yield

**Nice-to-have:**
- `image(url_or_obj)` — image output (less common in agent workflows)

All are straightforward to implement with rquickjs. The MVP includes all of them since the implementation is simple.

### 4.2 What We Do NOT Need

- Module loading / imports (code-mode blocks them)
- Node.js shims (console, fs, Buffer, etc.)
- SWC / TypeScript transpilation
- Filesystem access
- HTTP/fetch

---

## 5. Implementation Plan

### Step 1: Create the `wasi-code-runtime` crate

**New files:**

```
runtime/codex-wasm/wasi-code-runtime/
  Cargo.toml
  src/
    lib.rs          — spawn_runtime(), run_runtime(), RuntimeHandle
    globals.rs      — install_globals() with all code-mode globals
    value.rs        — JSON <-> rquickjs Value conversion helpers
```

**`Cargo.toml`:**

```toml
[package]
name = "wasi-code-runtime"
version = "0.1.0"
edition = "2021"

[workspace]

[dependencies]
rquickjs = { git = "https://github.com/DelSkayn/rquickjs.git", rev = "1fe498e7", features = [
    "futures",
    "macro",
] }
serde_json = "1"
futures-lite = "2"
```

Note: `loader` and `classes` features are not needed since code-mode blocks imports and does not define JS classes.

**`src/lib.rs` — Core runtime:**

```rust
//! rquickjs-based code-mode runtime for wasm32-wasip2.
//!
//! Replaces the V8 runtime stub. Provides spawn_runtime() with the same
//! signature that service.rs expects.

mod globals;
mod value;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt};
use serde_json::Value as JsonValue;

// These types are re-exported from the REPLACE_FILES stub in ast_transforms.rs.
// When wasi-code-runtime is used, the stub still defines the types but delegates
// spawn_runtime() to this crate.
//
// However, since the REPLACE_FILES entry replaces the ENTIRE mod.rs, we need to
// define the types here and have the replacement file re-export them.
// See Step 3 for the replacement file content.

use crate::description::ToolDefinition;
use crate::response::FunctionCallOutputContentItem;
use tokio::sync::mpsc;

// -- All type definitions (ExecuteRequest, RuntimeCommand, RuntimeEvent, etc.)
//    live in the REPLACE_FILES mod.rs and are identical to the current stub.
//    This crate only provides the runtime implementation functions.

/// Handle for terminating a running runtime.
#[derive(Clone)]
pub struct RuntimeHandle {
    terminate_flag: Arc<AtomicBool>,
}

impl RuntimeHandle {
    pub fn terminate_execution(&self) {
        self.terminate_flag.store(true, Ordering::SeqCst);
    }
}

pub fn spawn_runtime(
    request: ExecuteRequest,
    event_tx: mpsc::UnboundedSender<RuntimeEvent>,
) -> Result<(std::sync::mpsc::Sender<RuntimeCommand>, RuntimeHandle), String> {
    let (command_tx, command_rx) = std::sync::mpsc::channel();
    let terminate_flag = Arc::new(AtomicBool::new(false));
    let handle = RuntimeHandle {
        terminate_flag: terminate_flag.clone(),
    };

    tokio::spawn(run_runtime(request, event_tx, command_rx, terminate_flag));

    Ok((command_tx, handle))
}

async fn run_runtime(
    request: ExecuteRequest,
    event_tx: mpsc::UnboundedSender<RuntimeEvent>,
    command_rx: std::sync::mpsc::Receiver<RuntimeCommand>,
    terminate_flag: Arc<AtomicBool>,
) {
    // 1. Create runtime + context
    let runtime = match AsyncRuntime::new() {
        Ok(r) => r,
        Err(e) => {
            let _ = event_tx.send(RuntimeEvent::Result {
                stored_values: request.stored_values.clone(),
                error_text: Some(format!("failed to create JS runtime: {e}")),
            });
            return;
        }
    };

    // 2. Configure limits + interrupt handler
    configure_runtime(&runtime, &terminate_flag).await;

    let context = match AsyncContext::full(&runtime).await {
        Ok(c) => c,
        Err(e) => { /* send error, return */ }
    };

    // 3. Install globals
    let state = Arc::new(std::sync::Mutex::new(RuntimeState { ... }));
    context.with(|ctx| {
        globals::install_globals(&ctx, &request, &state)?;
        Ok::<_, rquickjs::Error>(())
    }).await;

    // 4. Send Started event
    let _ = event_tx.send(RuntimeEvent::Started);

    // 5. Evaluate source as script (not module — code-mode blocks imports)
    let eval_promise = context.with(|ctx| {
        let result = ctx.eval::<rquickjs::Value, _>(request.source.as_str());
        // Handle result, check for exit sentinel, capture promise if async
    }).await;

    // 6. Command loop: poll command_rx via try_recv(), drive microtasks
    loop {
        // Check termination
        if terminate_flag.load(Ordering::SeqCst) { break; }

        // Poll for commands (non-blocking)
        match command_rx.try_recv() {
            Ok(RuntimeCommand::ToolResponse { id, result }) => {
                // Resolve the stored promise resolver
            }
            Ok(RuntimeCommand::ToolError { id, error_text }) => {
                // Reject the stored promise resolver
            }
            Ok(RuntimeCommand::Terminate) => { break; }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => { break; }
        }

        // Drive microtasks
        if !runtime.execute_pending_job().await { break; }

        // Yield to cooperative scheduler
        tokio::task::yield_now().await;
    }

    // 7. Send Result event with stored values
}
```

**`src/globals.rs` — Global function installation:**

Each function is straightforward with rquickjs. The key pattern for `tools.<name>()`:

```rust
// For each enabled tool, create an async function that:
// 1. Serializes the input argument to JSON
// 2. Creates a Promise + resolver pair
// 3. Stores the resolver in RuntimeState.pending_resolvers
// 4. Sends RuntimeEvent::ToolCall
// 5. Returns the Promise

fn install_tool_function(ctx: &Ctx<'_>, tool: &ToolDefinition, state: &SharedState) {
    let tool_name = tool.tool_name.clone();
    let state = state.clone();
    let func = Function::new(ctx.clone(), move |ctx: Ctx<'_>, args: Rest<Value<'_>>| {
        let input = value::quickjs_to_json(&ctx, args.first())?;
        let (promise, resolve, reject) = ctx.promise()?;
        // Store resolve/reject, send event, return promise
    })?;
    // Set on tools object
}
```

**`src/value.rs` — JSON conversion:**

rquickjs has built-in serde support, so this is simpler than the V8 version:

```rust
pub fn quickjs_to_json(ctx: &Ctx<'_>, value: Option<&Value<'_>>) -> Result<Option<JsonValue>> {
    // Use ctx.json_stringify() or manual conversion
}

pub fn json_to_quickjs<'js>(ctx: &Ctx<'js>, value: &JsonValue) -> Result<Value<'js>> {
    // Use ctx.json_parse() or manual construction
}

pub fn serialize_output_text(ctx: &Ctx<'_>, value: Value<'_>) -> Result<String> {
    // Primitives: to_string(); Objects: JSON.stringify()
}
```

### Step 2: Wire the crate into the build

**`cargo_toml.rs` changes:**

Add `code-mode` to `INJECT_DEPS`:

```rust
const INJECT_DEPS: &[(&str, &[(&str, &str)])] = &[
    (
        "tui",
        &[
            ("codex-feedback", "../../../codex-wasm/wasi-codex-feedback"),
            ("codex-arg0", "../arg0"),
            ("codex-utils-sleep-inhibitor", "../utils/sleep-inhibitor"),
            ("arboard", "../../../codex-wasm/wasi-arboard"),
        ],
    ),
    (
        "code-mode",
        &[
            ("wasi-code-runtime", "../../../codex-wasm/wasi-code-runtime"),
        ],
    ),
];
```

This adds `wasi-code-runtime` as a dependency of the `code-mode` crate during the codemod pass.

The rquickjs git dependency in `wasi-code-runtime/Cargo.toml` resolves independently since the crate has its own `[workspace]` (same pattern as `wasi-sqlx`).

### Step 3: Update the REPLACE_FILES entry

Replace the current stub in `ast_transforms.rs` with a version that delegates to `wasi-code-runtime`:

```rust
(
    "code-mode/src/runtime/mod.rs",
    r#"//! rquickjs-based runtime for wasm32-wasip2.
//! Auto-generated by codex-codemod. Do not edit.

use std::collections::HashMap;
use serde_json::Value as JsonValue;
use tokio::sync::mpsc;
use crate::description::ToolDefinition;
use crate::response::FunctionCallOutputContentItem;

pub const DEFAULT_EXEC_YIELD_TIME_MS: u64 = 10_000;
pub const DEFAULT_WAIT_YIELD_TIME_MS: u64 = 10_000;
pub const DEFAULT_MAX_OUTPUT_TOKENS_PER_EXEC_CALL: usize = 10_000;

#[derive(Clone, Debug)]
pub struct ExecuteRequest {
    pub tool_call_id: String,
    pub enabled_tools: Vec<ToolDefinition>,
    pub source: String,
    pub stored_values: HashMap<String, JsonValue>,
    pub yield_time_ms: Option<u64>,
    pub max_output_tokens: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct WaitRequest {
    pub cell_id: String,
    pub yield_time_ms: u64,
    pub terminate: bool,
}

#[derive(Debug, PartialEq)]
pub enum RuntimeResponse {
    Yielded {
        cell_id: String,
        content_items: Vec<FunctionCallOutputContentItem>,
    },
    Terminated {
        cell_id: String,
        content_items: Vec<FunctionCallOutputContentItem>,
    },
    Result {
        cell_id: String,
        content_items: Vec<FunctionCallOutputContentItem>,
        stored_values: HashMap<String, JsonValue>,
        error_text: Option<String>,
    },
}

#[derive(Debug)]
pub(crate) enum TurnMessage {
    ToolCall {
        cell_id: String,
        id: String,
        name: String,
        input: Option<JsonValue>,
    },
    Notify {
        cell_id: String,
        call_id: String,
        text: String,
    },
}

#[derive(Debug)]
pub(crate) enum RuntimeCommand {
    ToolResponse { id: String, result: JsonValue },
    ToolError { id: String, error_text: String },
    Terminate,
}

#[derive(Debug)]
pub(crate) enum RuntimeEvent {
    Started,
    ContentItem(FunctionCallOutputContentItem),
    YieldRequested,
    ToolCall {
        id: String,
        name: String,
        input: Option<JsonValue>,
    },
    Notify {
        call_id: String,
        text: String,
    },
    Result {
        stored_values: HashMap<String, JsonValue>,
        error_text: Option<String>,
    },
}

/// Runtime handle backed by rquickjs interrupt handler.
#[derive(Clone)]
pub(crate) struct RuntimeHandle {
    inner: wasi_code_runtime::RuntimeHandle,
}

impl RuntimeHandle {
    pub fn terminate_execution(&self) {
        self.inner.terminate_execution();
    }
}

/// Spawn a rquickjs-based JavaScript runtime.
pub(crate) fn spawn_runtime(
    request: ExecuteRequest,
    event_tx: mpsc::UnboundedSender<RuntimeEvent>,
) -> Result<(std::sync::mpsc::Sender<RuntimeCommand>, RuntimeHandle), String> {
    let (tx, handle) = wasi_code_runtime::spawn_runtime(request, event_tx)?;
    Ok((tx, RuntimeHandle { inner: handle }))
}
"#,
),
```

**Critical detail:** The types (`ExecuteRequest`, `RuntimeCommand`, `RuntimeEvent`, etc.) must remain defined in `code-mode/src/runtime/mod.rs` because `service.rs` and the rest of the `code-mode` crate import them from `crate::runtime::*`. The `wasi-code-runtime` crate receives these types as function arguments.

**Design tension:** `wasi-code-runtime` needs to know about `ExecuteRequest`, `RuntimeCommand`, `RuntimeEvent`, and `FunctionCallOutputContentItem` — but these are defined inside `code-mode` which depends on `wasi-code-runtime`. This is a circular dependency.

### Step 3b: Resolving the Circular Dependency

**Option A: Duplicate types in `wasi-code-runtime`.**
Define matching types in `wasi-code-runtime` and convert at the boundary. Adds boilerplate but avoids architectural changes.

**Option B: Inline the runtime into the REPLACE_FILES entry.**
Instead of a separate crate, put the entire rquickjs runtime implementation directly in the `REPLACE_FILES` content (or `REPLACE_FILES_LARGE`). This avoids the circular dependency entirely because the code lives inside `code-mode`.

**Option C: Move types to a shared crate.**
Extract `ExecuteRequest`, `RuntimeCommand`, etc. into a `code-mode-types` crate that both `code-mode` and `wasi-code-runtime` depend on. Requires more codemod changes.

**Recommendation: Option B (inline).** The REPLACE_FILES mechanism already handles large file replacements (`REPLACE_FILES_LARGE` exists for this purpose). The rquickjs runtime implementation is ~400-600 lines — large but manageable. This is the simplest approach and follows the established pattern.

### Step 3 (revised): Inline Implementation via REPLACE_FILES

Instead of a separate crate, the REPLACE_FILES entry for `code-mode/src/runtime/mod.rs` contains the full rquickjs implementation. The `code-mode` crate's `Cargo.toml` gets `rquickjs` added directly.

**`cargo_toml.rs` changes:**

Extend `INJECT_DEPS` to add rquickjs to code-mode. However, `INJECT_DEPS` currently only supports path-based deps. We need a new mechanism for git deps, OR we add rquickjs to the workspace `Cargo.toml` and reference it via `workspace = true`.

**Simpler approach:** Add a `PER_CRATE_ADD_DEPS` mechanism that injects arbitrary TOML dep entries:

```rust
const PER_CRATE_ADD_DEPS: &[(&str, &[(&str, &str)])] = &[
    (
        "code-mode",
        &[
            ("rquickjs", r#"{ git = "https://github.com/DelSkayn/rquickjs.git", rev = "1fe498e7", features = ["futures", "macro"] }"#),
            ("futures-lite", r#""2""#),
        ],
    ),
];
```

**Simplest approach:** Since the submodule carries pre-applied codemod output, we can also just manually add the dependency to the patched `code-mode/Cargo.toml` in the submodule. But this breaks the "codemod is the source of truth" principle.

**Best approach:** Use the existing `INJECT_DEPS` pattern but with a wrapper crate. Create `runtime/codex-wasm/wasi-code-runtime/` as a thin crate that re-exports rquickjs and futures-lite. The `code-mode` crate depends on `wasi-code-runtime` via `INJECT_DEPS`, and the REPLACE_FILES mod.rs `use`s rquickjs through it.

```toml
# wasi-code-runtime/Cargo.toml
[package]
name = "wasi-code-runtime"
version = "0.1.0"
edition = "2021"

[workspace]

[dependencies]
rquickjs = { git = "https://github.com/DelSkayn/rquickjs.git", rev = "1fe498e7", features = [
    "futures",
    "macro",
] }
futures-lite = "2"

[lib]
# Re-export everything
```

```rust
// wasi-code-runtime/src/lib.rs
pub use rquickjs;
pub use futures_lite;
```

Then in the REPLACE_FILES mod.rs:
```rust
use wasi_code_runtime::rquickjs::{AsyncRuntime, AsyncContext, ...};
use wasi_code_runtime::futures_lite;
```

This cleanly separates the git dependency management from the code-mode implementation.

---

## 6. Revised Architecture

```
code-mode/src/
  lib.rs              — public API (unchanged)
  description.rs      — tool descriptions (unchanged)
  response.rs         — FunctionCallOutputContentItem (unchanged)
  service.rs          — CodeModeService (unchanged, codemod handles v8::IsolateHandle)
  runtime/
    mod.rs            — REPLACED by REPLACE_FILES_LARGE entry:
                        - All type definitions (ExecuteRequest, RuntimeCommand, etc.)
                        - RuntimeHandle backed by AtomicBool
                        - spawn_runtime() using tokio::spawn
                        - run_runtime() async fn with rquickjs
                        - install_globals() for code-mode globals
                        - JSON value helpers
                        globals.rs      — REPLACED with empty file (merged into mod.rs)
                        callbacks.rs    — REPLACED with empty file (merged into mod.rs)
                        value.rs        — REPLACED with empty file (merged into mod.rs)
                        module_loader.rs — REPLACED with empty file (merged into mod.rs)
```

The key simplification: V8 spreads across 5 files because of its complex scope/lifetime system. rquickjs is much simpler and everything fits in a single mod.rs.

---

## 7. Detailed Implementation: `code-mode/src/runtime/mod.rs`

This is the full REPLACE_FILES content. The implementation follows tsx-engine patterns for rquickjs setup and code-mode patterns for the tool call protocol.

### 7.1 Runtime State

```rust
struct RuntimeState {
    event_tx: mpsc::UnboundedSender<RuntimeEvent>,
    tool_call_id: String,
    stored_values: HashMap<String, JsonValue>,
    next_tool_call_id: u64,
    pending_resolvers: HashMap<String, (rquickjs::Function<'static>, rquickjs::Function<'static>)>,
    exit_requested: bool,
}
```

Note: rquickjs promise resolvers are `Function` objects (resolve/reject callbacks), not `PromiseResolver` like V8. The `Promise::new()` API returns `(Promise, resolve_fn, reject_fn)`. Since these must outlive the `with()` closure, they need to be stored as `Persistent<Function>` or the state must be accessed within `context.with()`.

**Revised approach:** Store resolvers as `rquickjs::Persistent<rquickjs::Function>` pairs, or use a different mechanism. Actually, rquickjs provides `rquickjs::promise::Promised` and `ctx.promise()` which returns `(Promise, resolve, reject)` where resolve/reject are `Function` values. These can be stored as `Persistent` values.

### 7.2 The Command Loop Problem

The V8 version uses `command_rx.recv()` (blocking) on a dedicated OS thread. In WASM:

1. `std::sync::mpsc::Receiver::recv()` will block the single thread forever
2. `std::sync::mpsc::Receiver::try_recv()` is non-blocking but needs polling
3. We need to interleave: drive microtasks, check for commands, yield to scheduler

**Solution:** Use `try_recv()` in an async loop with `tokio::task::yield_now()`:

```rust
loop {
    // 1. Process all available commands
    loop {
        match command_rx.try_recv() {
            Ok(cmd) => handle_command(cmd, &context, &state).await,
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => return,
        }
    }

    // 2. Drive one pending microtask
    let has_pending = runtime.execute_pending_job().await;

    // 3. Check completion
    if !has_pending && state.lock().unwrap().pending_resolvers.is_empty() {
        break; // Script complete, no pending tool calls
    }

    // 4. Check termination
    if terminate_flag.load(Ordering::SeqCst) { break; }

    // 5. Yield to cooperative scheduler
    tokio::task::yield_now().await;
}
```

### 7.3 Promise Resolution Within `context.with()`

rquickjs requires all JS value manipulation to happen inside `context.with()`. Resolving a stored promise means:

```rust
async fn resolve_tool_response(
    context: &AsyncContext,
    state: &Arc<Mutex<RuntimeState>>,
    id: &str,
    result: JsonValue,
) {
    context.with(|ctx| {
        let resolver = state.lock().unwrap().pending_resolvers.remove(id);
        if let Some((resolve_fn, _reject_fn)) = resolver {
            let value = json_to_quickjs(&ctx, &result)?;
            resolve_fn.call::<_, ()>((value,))?;
        }
        Ok::<_, rquickjs::Error>(())
    }).await;
}
```

### 7.4 Module Evaluation

Code-mode evaluates source as an ES module (V8's `compile_module` + `evaluate`). With rquickjs:

```rust
context.with(|ctx| {
    // Use Module API to evaluate as ES module
    let module = Module::declare(ctx.clone(), "exec_main.mjs", &request.source)?;
    let (module, promise) = module.eval()?;
    // promise represents the module's completion
    Ok::<_, rquickjs::Error>(promise)
}).await
```

However, since code-mode blocks all imports, we could also just use `ctx.eval()` (script mode). The upstream uses modules primarily for the `resolve_module_callback` that rejects imports. With rquickjs, we can configure the loader to reject all imports instead.

**Decision:** Use `ctx.eval()` (script mode) for simplicity. If we later need module semantics (e.g., top-level await), switch to `Module::declare().eval()`. Both work with rquickjs on WASM.

---

## 8. Implementation Steps (Concrete)

### Phase 1: Dependency Crate + Build Wiring

1. **Create `runtime/codex-wasm/wasi-code-runtime/`**
   - `Cargo.toml` with rquickjs (git, rev 1fe498e7, features: futures, macro) + futures-lite
   - `src/lib.rs` with `pub use rquickjs; pub use futures_lite;`

2. **Update `cargo_toml.rs`**
   - Add `("code-mode", &[("wasi-code-runtime", "../../../codex-wasm/wasi-code-runtime")])` to `INJECT_DEPS`

3. **Verify build** — run `moon run runtime:build-wasm` to confirm the dependency resolves

### Phase 2: Replace the Stub

4. **Write the full rquickjs implementation** as a `REPLACE_FILES_LARGE` entry in `ast_transforms.rs`
   - All existing type definitions (copy from current stub)
   - `RuntimeHandle` with `AtomicBool` terminate flag
   - `spawn_runtime()` using `tokio::spawn`
   - `run_runtime()` async function
   - `install_globals()` for all 9 globals
   - `json_to_quickjs()` / `quickjs_to_json()` / `serialize_output_text()` helpers

5. **Add REPLACE_FILES entries for the now-unused V8 files:**
   ```rust
   ("code-mode/src/runtime/globals.rs", "// Merged into mod.rs for rquickjs build.\n"),
   ("code-mode/src/runtime/callbacks.rs", "// Merged into mod.rs for rquickjs build.\n"),
   ("code-mode/src/runtime/value.rs", "// Merged into mod.rs for rquickjs build.\n"),
   ("code-mode/src/runtime/module_loader.rs", "// Merged into mod.rs for rquickjs build.\n"),
   ```

6. **Verify build** — `moon run runtime:build-wasm`

### Phase 3: Integration Testing

7. **Test with the TUI** — `pnpm dev`, use the `exec` tool from the agent
8. **Test tool calls** — `await tools.exec_command({ command: ["echo", "hello"] })`
9. **Test store/load** — `store("x", 42)` then `load("x")` in a new exec cell
10. **Test exit** — `exit()` in the middle of a script
11. **Test termination** — long-running script, terminate via `wait({terminate: true})`

### Phase 4: E2E Tests

12. Add Playwright E2E test for `exec` tool in the browser TUI
13. Add vitest unit test for the rquickjs globals (if testable outside WASM)

---

## 9. Files to Create/Modify

| File | Action | Purpose |
|------|--------|---------|
| `runtime/codex-wasm/wasi-code-runtime/Cargo.toml` | **Create** | Thin wrapper re-exporting rquickjs + futures-lite |
| `runtime/codex-wasm/wasi-code-runtime/src/lib.rs` | **Create** | `pub use rquickjs; pub use futures_lite;` |
| `runtime/codex-wasm/codex-codemod/src/cargo_toml.rs` | **Modify** | Add code-mode to INJECT_DEPS |
| `runtime/codex-wasm/codex-codemod/src/ast_transforms.rs` | **Modify** | Replace stub with rquickjs impl in REPLACE_FILES_LARGE |

---

## 10. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| rquickjs Promise API mismatch | Low | Medium | tsx-engine proves the API works; test promise resolution flow early |
| `std::sync::mpsc::try_recv()` polling latency | Medium | Low | `tokio::task::yield_now()` keeps the loop responsive; can add sleep(1ms) if needed |
| Persistent JS values across `context.with()` calls | Medium | Medium | Use `rquickjs::Persistent<T>` to store promise resolve/reject functions |
| REPLACE_FILES_LARGE content too big for inline string | Low | Low | Can use `include_str!()` if needed |
| Binary size increase | Low | Low | rquickjs adds ~600KB-1MB; acceptable for restored exec functionality |
| Upstream code-mode API changes | Medium | Medium | Codemod isolates us; monitor upstream for changes to service.rs interface |

---

## 11. Estimated Effort

| Phase | Effort | Dependencies |
|-------|--------|-------------|
| Phase 1: Dependency crate + build wiring | 0.5 day | None |
| Phase 2: Replace stub with rquickjs impl | 2-3 days | Phase 1 |
| Phase 3: Integration testing | 1-2 days | Phase 2 |
| Phase 4: E2E tests | 0.5-1 day | Phase 3 |
| **Total** | **4-6.5 days** | |

---

## 12. Key Files Reference

| File | Role |
|------|------|
| `runtime/codex-upstream/codex-rs/code-mode/src/runtime/mod.rs` | Current stub (post-codemod) |
| `runtime/codex-upstream/codex-rs/code-mode/src/runtime/globals.rs` | Original V8 global installation (reference) |
| `runtime/codex-upstream/codex-rs/code-mode/src/runtime/callbacks.rs` | Original V8 callbacks (reference for behavior) |
| `runtime/codex-upstream/codex-rs/code-mode/src/runtime/value.rs` | Original V8 JSON helpers (reference) |
| `runtime/codex-upstream/codex-rs/code-mode/src/runtime/module_loader.rs` | Original V8 module eval (reference) |
| `runtime/codex-upstream/codex-rs/code-mode/src/service.rs` | Service layer — our spawn_runtime() must match its expectations |
| `runtime/codex-upstream/codex-rs/code-mode/src/response.rs` | FunctionCallOutputContentItem, ImageDetail types |
| `runtime/codex-upstream/codex-rs/code-mode/src/description.rs` | ToolDefinition type, exec tool description |
| `runtime/codex-wasm/codex-codemod/src/ast_transforms.rs` | REPLACE_FILES with current stub |
| `runtime/codex-wasm/codex-codemod/src/syn_transforms.rs` | v8::IsolateHandle -> RuntimeHandle rewrite |
| `runtime/codex-wasm/codex-codemod/src/cargo_toml.rs` | INJECT_DEPS, STRIP_DEPS |
| `runtime/crates/tsx-engine/src/lib.rs` | Reference rquickjs setup (runtime, limits, eval) |
| `runtime/crates/tsx-engine/Cargo.toml` | rquickjs git rev + features |

---

## 13. Open Questions

1. **`ctx.eval()` vs `Module::declare().eval()`** — The upstream uses ES module evaluation. Should we match this exactly, or is script-mode eval sufficient? Script mode is simpler but does not support top-level `await`. If the LLM generates `const result = await tools.foo()` at the top level, module mode is required.

   **Answer: Use module mode.** The exec tool description explicitly shows `await tools.exec_command(...)` at the top level. ES module evaluation supports top-level await; script mode does not. Use `Module::declare(ctx, "exec_main.mjs", &source)?.eval()`.

2. **`console` removal** — The V8 version explicitly deletes `console` from `globalThis`. Should we do the same? The exec tool description says "no console". rquickjs does not install `console` by default (unlike V8), so this may be automatic.

   **Answer: Verify and delete if present.** `AsyncContext::full()` may install console. Check and remove if so.

3. **rquickjs `Persistent` for promise functions** — Can `rquickjs::Function` be stored outside `context.with()` closures? Need to verify the `Persistent<T>` API.

   **Answer: Yes.** rquickjs provides `Persistent<T>` (also called `OwnedValue` in some versions) for storing JS values outside context scopes. The tsx-engine does not use this pattern, so it needs validation.
