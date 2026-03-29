# Clipboard Support for Codex TUI WASM Build

## Status: Planned

## Problem

The Codex TUI runs in the browser via WASM. Clipboard operations (copy/paste) are
currently stubbed out:

1. **wasi-arboard crate replacement** (`runtime/codex-wasm/wasi-arboard/src/lib.rs`):
   The codemod in `cargo_toml.rs` redirects the `arboard` dependency to this local
   crate via `INJECT_DEPS` (line 92: `("arboard", "../../../codex-wasm/wasi-arboard")`).
   The stub always returns `Err(Error::ClipboardNotSupported)` from `Clipboard::new()`,
   `get_text()`, and `set_text()`.

2. **syn_transforms STRING_REPLACEMENT** (`runtime/codex-wasm/codex-codemod/src/syn_transforms.rs`,
   line 1567): The codemod replaces the `arboard::Clipboard::new()` match chain in
   `tui/src/clipboard_text.rs` with a hardcoded error string
   `"clipboard not available in WASM"`. This was applied because the stub's
   `Clipboard::new()` returned `Err`, making the original match arms dead code that
   the compiler would warn about.

3. **clipboard_paste.rs**: Uses `arboard::Clipboard::new()` for image paste. The
   stub causes `paste_image_as_png()` to always fail with `ClipboardUnavailable`.

The browser has the Clipboard API (`navigator.clipboard.writeText/readText`) that
can bridge this gap. This plan follows the exact same WIT interface pattern used for
WebSocket and shell-exec support.

## Architecture Overview

This follows the established pattern used by `websocket` and `shell-exec`:

```
Rust (WASM)                         JS (Browser)
-----------                         -----------
codex-wasm-tui/src/lib.rs           clipboard-impl.ts (JS shim)
  calls crate::bindings::            JCO maps WIT import to
  codex::tui::clipboard       --->     @tjfontaine/wasi-shims/clipboard-impl.js
    ::write_text(text)         --->     navigator.clipboard.writeText()
    ::read_text()              --->     navigator.clipboard.readText()

WIT (world.wit)                     Transpile (transpile.mjs)
  interface clipboard { ... }         --map 'codex:tui/clipboard@0.1.0=...'
  import clipboard;                   --async-imports 'codex:tui/clipboard@0.1.0#...'
```

### Comparison with existing patterns

| Aspect | shell-exec | websocket | clipboard (planned) |
|--------|-----------|-----------|-------------------|
| WIT file | `world.wit` | `world.wit` | `world.wit` |
| JS shim | `shell-exec-impl.ts` | `websocket-impl.ts` | `clipboard-impl.ts` |
| Rust backend | N/A (host-only) | `websocket_backend.rs` | Direct WIT binding calls |
| JSPI async funcs | `exec` | `connect`, `recv` | `write-text`, `read-text` |
| Shim registration | `setExecHandler()` | N/A (self-contained) | N/A (self-contained) |

## Implementation Plan

### Step 1: WIT Interface Definition

**File:** `runtime/codex-wasm/codex-wasm-tui/wit/world.wit`

Add a `clipboard` interface alongside the existing `shell-exec` and `websocket`
interfaces:

```wit
/// Clipboard interface -- the host (browser) bridges to navigator.clipboard API.
interface clipboard {
    /// Write text to the system clipboard.
    /// Returns an error string if the operation fails (e.g., permission denied).
    write-text: func(text: string) -> result<_, string>;

    /// Read text from the system clipboard.
    /// Returns the clipboard text, or an error string if unavailable.
    read-text: func() -> result<string, string>;
}
```

Then add the import to the `codex-tui` world:

```wit
world codex-tui {
    // ... existing imports ...

    // Shell execution -- routes to host sandbox
    import shell-exec;

    // WebSocket -- routes to browser native WebSocket API
    import websocket;

    // Clipboard -- routes to browser Clipboard API
    import clipboard;

    // ... rest unchanged ...
    export run: func() -> s32;
}
```

**Design notes:**
- Text-only for Phase 1. Image clipboard can be added later for `clipboard_paste.rs`.
- Both functions return `result` to propagate permission errors gracefully.
- Both must be async (JSPI) because `navigator.clipboard` methods return Promises.

### Step 2: JS Shim Implementation

**File:** `packages/wasi-shims/src/clipboard-impl.ts`

Follow the self-contained pattern of `websocket-impl.ts` (no handler registration
needed, unlike `shell-exec-impl.ts` which requires `setExecHandler()`):

```typescript
/**
 * Clipboard implementation for codex:tui/clipboard WIT interface.
 *
 * Bridges the Codex TUI WASM clipboard calls to the browser's
 * navigator.clipboard API (Clipboard API Level 2).
 *
 * Note on Worker context: The Clipboard API is NOT available in
 * SharedWorker or DedicatedWorker contexts. This shim must relay
 * clipboard operations to the main window thread via the existing
 * worker-bridge infrastructure. See "Worker Clipboard Access" below.
 */

/**
 * Write text to the system clipboard.
 * JCO maps codex:tui/clipboard@0.1.0#write-text to this function.
 * Returns a Promise (JSPI-suspending) that resolves on success.
 * On error, throw and JCO wraps in {tag:'err'}.
 */
export async function writeText(text: string): Promise<void> {
    // TODO: Implement worker-to-main-thread relay (see below)
    throw new Error('Clipboard API not yet implemented');
}

/**
 * Read text from the system clipboard.
 * JCO maps codex:tui/clipboard@0.1.0#read-text to this function.
 * Returns a Promise (JSPI-suspending) that resolves with the text.
 */
export async function readText(): Promise<string> {
    // TODO: Implement worker-to-main-thread relay (see below)
    throw new Error('Clipboard API not yet implemented');
}
```

### Step 3: Update wasi-arboard to Call WIT Bindings

**File:** `runtime/codex-wasm/wasi-arboard/src/lib.rs`

Since all crates compile into a single WASM component, the WIT bindings generated
by `wit-bindgen` in `codex-wasm-tui` are available to all crates in the component.
The wasi-arboard crate should call these bindings via a global function pointer
registered at startup by the TUI crate.

This is needed because wasi-arboard cannot directly `use` the `bindings` module
from codex-wasm-tui (it would create a circular dependency). The function pointer
pattern is straightforward:

```rust
#![allow(dead_code, unused_variables)]

use std::fmt;
use std::sync::OnceLock;

type ClipboardWriteFn = fn(&str) -> Result<(), String>;
type ClipboardReadFn = fn() -> Result<String, String>;

static WRITE_FN: OnceLock<ClipboardWriteFn> = OnceLock::new();
static READ_FN: OnceLock<ClipboardReadFn> = OnceLock::new();

/// Called by codex-wasm-tui at startup to register the WIT clipboard backend.
pub fn register_clipboard_backend(
    write_fn: ClipboardWriteFn,
    read_fn: ClipboardReadFn,
) {
    let _ = WRITE_FN.set(write_fn);
    let _ = READ_FN.set(read_fn);
}

pub struct Clipboard;

impl Clipboard {
    pub fn new() -> Result<Self, Error> {
        // Succeed if backend is registered, fail otherwise
        if READ_FN.get().is_some() {
            Ok(Clipboard)
        } else {
            Err(Error::ClipboardNotSupported)
        }
    }

    pub fn get_text(&mut self) -> Result<String, Error> {
        match READ_FN.get() {
            Some(f) => f().map_err(|e| Error::Unknown(e)),
            None => Err(Error::ClipboardNotSupported),
        }
    }

    pub fn set_text(&mut self, text: String) -> Result<(), Error> {
        match WRITE_FN.get() {
            Some(f) => f(&text).map_err(|e| Error::Unknown(e)),
            None => Err(Error::ClipboardNotSupported),
        }
    }

    pub fn get(&mut self) -> Get<'_> {
        Get { _clipboard: self }
    }

    pub fn get_image(&mut self) -> Result<ImageData<'static>, Error> {
        // Image clipboard: Phase 2
        Err(Error::ClipboardNotSupported)
    }
}

// ... rest unchanged (Get, ImageData, Error types) ...
```

### Step 4: Register WIT Clipboard in codex-wasm-tui

**File:** `runtime/codex-wasm/codex-wasm-tui/src/lib.rs` (or new `clipboard_backend.rs`)

Follow the pattern of `websocket_backend.rs` -- import from `crate::bindings` and
wire up at startup:

```rust
// clipboard_backend.rs
use crate::bindings::codex::tui::clipboard;

fn wit_clipboard_write(text: &str) -> Result<(), String> {
    clipboard::write_text(text)
}

fn wit_clipboard_read() -> Result<String, String> {
    clipboard::read_text()
}

pub fn init() {
    arboard::register_clipboard_backend(wit_clipboard_write, wit_clipboard_read);
}
```

Call `clipboard_backend::init()` early in the TUI startup path (in the `run()`
export), before any clipboard operations occur. This is analogous to how
`websocket_backend.rs` provides `WasiWebSocketBackend` for the WebSocket trait.

### Step 5: Remove syn_transforms Clipboard Stub

**File:** `runtime/codex-wasm/codex-codemod/src/syn_transforms.rs` (around line 1567)

Remove the `string_replace` call that stubs out the arboard match chain in
`clipboard_text.rs`:

```rust
// REMOVE this block:
// --- tui/src/clipboard_text.rs: stub arboard clipboard ---
self.string_replace(
    "tui/src/clipboard_text.rs",
    "    let error = match arboard::Clipboard::new() { ... }",
    "    let error = \"clipboard not available in WASM\".to_string();",
);
```

With the wasi-arboard crate now delegating to the WIT backend, the original
upstream code works as-is: `arboard::Clipboard::new()` succeeds (backend is
registered), `clipboard.set_text()` calls through to the browser Clipboard API.

**Also remove** the corresponding entry in `ast_transforms.rs` (line 250) if it
still exists -- both the syn-based and string-based transforms have this stub.

**After removing:** Re-apply the codemod to the submodule and verify the upstream
`clipboard_text.rs` compiles without warnings. The original code:

```rust
let error = match arboard::Clipboard::new() {
    Ok(mut clipboard) => match clipboard.set_text(text.to_string()) {
        Ok(()) => return Ok(()),
        Err(err) => format!("clipboard unavailable: {err}"),
    },
    Err(err) => format!("clipboard unavailable: {err}"),
};
```

...will now execute the `Ok` path since `Clipboard::new()` succeeds when the
backend is registered.

### Step 6: Transpile Config Changes

**File:** `scripts/transpile.mjs`

Add the clipboard shim to the `codex-wasm-tui` module entry (around line 217):

```javascript
'codex-wasm-tui': {
    wasm: 'codex_wasm_tui.wasm',
    jspiOut: `${FRONTEND}/src/wasm/codex-tui`,
    syncOut: `${FRONTEND}/src/wasm/codex-tui-sync`,
    shims: {
        ...SHIMS,
        'codex:tui/shell-exec@0.1.0': '@tjfontaine/wasi-shims/shell-exec-impl.js',
        'codex:tui/websocket@0.1.0': '@tjfontaine/wasi-shims/websocket-impl.js',
        'codex:tui/clipboard@0.1.0': '@tjfontaine/wasi-shims/clipboard-impl.js',  // NEW
    },
    exports: ['run'],
    extraAsyncImports: [
        'codex:tui/shell-exec@0.1.0#exec',
        'codex:tui/websocket@0.1.0#connect',
        'codex:tui/websocket@0.1.0#recv',
        'codex:tui/clipboard@0.1.0#write-text',   // NEW
        'codex:tui/clipboard@0.1.0#read-text',    // NEW
        'wasi:io/streams@0.2.9#[method]input-stream.blocking-read',
        'wasi:http/outgoing-handler@0.2.9#handle',
    ],
},
```

Both `write-text` and `read-text` must be in `extraAsyncImports` because the
browser Clipboard API is Promise-based and requires JSPI suspension.

### Step 7: Vite Config Changes

**File:** `frontend/vite.config.ts`

Add `clipboard-impl.js` in the same places as `websocket-impl.js` and
`shell-exec-impl.js`. There are two `output.paths` blocks (worker and build) that
both need the new entry:

1. **`resolve.dedupe`** array (around line 43):
```javascript
'@tjfontaine/wasi-shims/clipboard-impl.js',
```

2. **`worker.rollupOptions.output.paths`** (around line 237):
```javascript
'@tjfontaine/wasi-shims/clipboard-impl.js': '/wasi-shims/clipboard-impl.js',
```

3. **`build.rollupOptions.output.paths`** (around line 322):
```javascript
'@tjfontaine/wasi-shims/clipboard-impl.js': '/wasi-shims/clipboard-impl.js',
```

These mappings ensure that the JCO-generated `import` for the clipboard shim
resolves to the correct `/wasi-shims/clipboard-impl.js` path at runtime, both in
the SharedWorker (rollup worker paths) and the production build (rollup build paths).

### Step 8: Package Export (Optional)

**File:** `packages/wasi-shims/package.json`

Note: The existing `websocket-impl.js` and `shell-exec-impl.js` shims are NOT
listed in the `package.json` exports. They resolve at runtime via Vite's
`rollupOptions.output.paths` mapping and the `copy-externals` Moon task that copies
`browser-dist/**` to `dist/wasi-shims/`. The `build-browser.mjs` script bundles
ALL `dist/*.js` files into `browser-dist/`.

Since `clipboard-impl.ts` compiles to `dist/clipboard-impl.js` via `tsc`, and
`build-browser.mjs` picks up all `dist/*.js` files automatically, **no changes to
`package.json` are needed** -- the existing build pipeline handles it.

## Worker Clipboard Access

The WASM TUI runs inside a SharedWorker. This is the critical constraint:

### The Problem

`navigator.clipboard` is **NOT available in Worker contexts** (SharedWorker or
DedicatedWorker). The Clipboard API is a window-only API that requires:
- A secure context (HTTPS or localhost)
- A focused document for `readText()`
- User activation (recent user gesture) for some operations

The SharedWorker has none of these.

### The Solution: Main Thread Relay

The clipboard shim must post a message to the main window thread and await the
response. The existing worker communication infrastructure can be extended:

1. **SharedWorker side** (`clipboard-impl.ts`): Send a `clipboard:writeText` or
   `clipboard:readText` message via the worker's MessagePort to the main thread.
   Return a Promise that resolves when the main thread responds.

2. **Main thread side** (`frontend/src/`): Listen for clipboard messages from the
   worker. Execute `navigator.clipboard.writeText()`/`readText()` in the main
   thread where the Clipboard API is available, then post the result back.

This is similar to how `shell-exec-impl.ts` uses `setExecHandler()` to delegate
to the host -- but for clipboard, the delegation crosses the Worker/main-thread
boundary rather than being set up programmatically.

### User Activation Constraints

- **Write (copy):** `navigator.clipboard.writeText()` works with the
  `"clipboard-write"` permission, which browsers typically auto-grant.
  The main thread should have user activation propagated from the keyboard
  event that triggered `/copy`.

- **Read (paste):** `navigator.clipboard.readText()` requires both user activation
  and the `"clipboard-read"` permission. The browser will show a permission prompt
  on first use. This is the harder case -- the user activation from the Worker's
  keyboard event may not propagate to the main thread's clipboard call.

- **Fallback:** If the Clipboard API is unavailable or permission is denied, the
  shim throws an error string that propagates through the WIT `result<_, string>`
  type. The TUI displays this to the user. The existing OSC 52 fallback in
  `clipboard_text.rs` for SSH sessions is checked before arboard, so that path
  is unaffected.

### Permissions Policy

- Ensure the page's `Permissions-Policy` header does not block `clipboard-read`
  or `clipboard-write`.
- The existing COOP/COEP headers for SharedArrayBuffer should not interfere.

## Testing Approach

### Unit Tests (Rust)
- Test that `arboard::Clipboard::new()` succeeds after `register_clipboard_backend()`.
- Test that `set_text()`/`get_text()` delegate to registered functions.
- Test that operations return `ClipboardNotSupported` before registration.

### Integration Tests (JS shim)
- Test `writeText()`/`readText()` with a mock Worker MessagePort.
- Test error propagation from permission denial.

### E2E Tests (Playwright)
- Playwright supports `browserContext.grantPermissions(['clipboard-read', 'clipboard-write'])`.
- Test the `/copy` command in the TUI and verify clipboard contents.
- Test paste operations with pre-populated clipboard.
- Test permission denial scenario.

### Build Validation
```sh
moon run runtime:build-wasm          # Verify WASM compiles with new WIT interface
moon run frontend:build              # Verify transpile + Vite build succeeds
moon run frontend:copy-externals     # Verify clipboard-impl.js is in dist/wasi-shims/
moon run frontend:test               # Unit tests
moon run frontend:test-e2e           # E2E clipboard tests
```

## Dependencies and Ordering

```
1. WIT interface definition (world.wit)
2. JS shim (clipboard-impl.ts)                    -- parallel with 1
3. wasi-arboard changes (function pointer pattern) -- after 1 (needs WIT to compile)
4. codex-wasm-tui registration (clipboard_backend.rs) -- after 3
5. Remove syn_transforms clipboard stub            -- after 3
6. transpile.mjs config                            -- after 1
7. vite.config.ts paths                            -- parallel with 6
8. Build and test: moon run runtime:build-wasm && pnpm build
```

Steps 1-2 can be done in parallel. Steps 3-5 are sequential. Steps 6-7 can be
done in parallel after step 1.

## File Checklist

| File | Change |
|------|--------|
| `runtime/codex-wasm/codex-wasm-tui/wit/world.wit` | Add `interface clipboard` + `import clipboard` |
| `packages/wasi-shims/src/clipboard-impl.ts` | New file: JS shim with main-thread relay |
| `runtime/codex-wasm/wasi-arboard/src/lib.rs` | Add `register_clipboard_backend()` + delegate calls |
| `runtime/codex-wasm/codex-wasm-tui/src/clipboard_backend.rs` | New file: WIT binding wrappers + `init()` |
| `runtime/codex-wasm/codex-wasm-tui/src/lib.rs` | Call `clipboard_backend::init()` at startup |
| `runtime/codex-wasm/codex-codemod/src/syn_transforms.rs` | Remove clipboard_text.rs string_replace (line ~1567) |
| `runtime/codex-wasm/codex-codemod/src/ast_transforms.rs` | Remove clipboard stub STRING_REPLACEMENT (line ~250) |
| `scripts/transpile.mjs` | Add clipboard shim + async imports to codex-wasm-tui |
| `frontend/vite.config.ts` | Add clipboard-impl.js to dedupe + both paths blocks |

**Files that do NOT need changes:**
- `packages/wasi-shims/package.json` -- build-browser.mjs auto-discovers all dist/*.js
- `runtime/codex-wasm/wasi-arboard/Cargo.toml` -- no new dependencies needed
- `runtime/codex-wasm/codex-codemod/src/cargo_toml.rs` -- arboard redirect already in INJECT_DEPS

## Future Work (Phase 2)

- **Image clipboard:** Add `read-image`/`write-image` to the WIT interface for
  `clipboard_paste.rs` image paste support. The browser Clipboard API supports
  `ClipboardItem` with `image/png` MIME type.
- **Permissions UI:** Surface clipboard permission state in the TUI status bar so
  users know why clipboard operations fail.
- **iOS bridge:** For the iOS build, the clipboard WIT interface could route to
  `UIPasteboard` instead of `navigator.clipboard`, following the same pattern as
  `ios:bridge/*` stubs.
