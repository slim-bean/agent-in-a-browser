# File Search in the Browser (WASM)

## What File Search Does

The Codex TUI has a fuzzy file finder triggered by typing `@` in the chat composer.
As the user types (e.g. `@main.rs`), the TUI performs a live fuzzy search over every
file in the workspace, displays a ranked popup of matches with highlighted characters,
and lets the user select a file to insert its path into the prompt. This is the
equivalent of Ctrl+P / "Go to File" in VS Code.

### Why It Matters

- **Context attachment**: Users type `@filename` to reference files in conversations
  with the agent. Without file search, users must type exact paths manually.
- **Discoverability**: In an unfamiliar project cloned into the browser sandbox, fuzzy
  file search is the primary way users navigate the workspace.
- **Parity with native**: The native Codex TUI has this feature working. The browser
  version currently bails with an error, creating a visible gap.

## Current State: Stubbed Out

### The Bail

In `runtime/codex-upstream/codex-rs/file-search/src/lib.rs`, the codemod
(`runtime/codex-wasm/codex-codemod/src/syn_transforms.rs`) injects a compile-time
bail at the top of `create_session()`:

```rust
#[cfg(target_arch = "wasm32")]
{
    let _ = (&search_directories, &options, &reporter, &cancel_flag);
    anyhow::bail!("File search is not available in the browser (requires OS threads)");
}
```

### What Breaks

When a user types `@` in the chat composer:
1. `ChatComposer` sends `AppEvent::StartFileSearch(query)` on every keystroke
2. `App::file_search.on_user_query()` tries to create a `FileSearchSession`
3. `create_session()` returns `Err(...)` on wasm32
4. `FileSearchManager::start_session_locked()` logs a warning and sets `session = None`
5. The popup shows "loading..." indefinitely -- no results ever arrive

The user sees a broken popup with no feedback about why search does not work.

### Dependencies Involved

The `file-search` crate depends on three problematic crates:

| Crate | Purpose | WASM Problem |
|-------|---------|-------------|
| `nucleo` | Fuzzy matching engine (from Helix editor) | Spawns matcher threads internally (`Nucleo::new` takes thread count) |
| `crossbeam-channel` | Multi-producer channels for worker coordination | Uses OS-level synchronization primitives |
| `ignore` | Gitignore-aware directory walker (from ripgrep) | `build_parallel()` spawns OS threads; uses `walkdir` internally |

Note: These crates are NOT stripped by the codemod -- they compile for wasm32-wasip2
(since `tokio::thread_spawn` shims thread creation). The problem is they do not
function correctly at runtime in single-threaded WASM.

## How Upstream File Search Works

### Architecture (Two Background Threads)

```
create_session()
  |
  +-- walker_worker (thread 1)
  |     Uses ignore::WalkBuilder::build_parallel()
  |     Feeds discovered paths into nucleo via Injector
  |     Sends WorkSignal::WalkComplete when done
  |
  +-- matcher_worker (thread 2)
  |     Owns the Nucleo instance
  |     Receives WorkSignal::{QueryUpdated, NucleoNotify, WalkComplete}
  |     via crossbeam_channel::select! loop
  |     On each tick: calls nucleo.tick(), extracts top-N matches
  |     Reports snapshots via SessionReporter::on_update()
  |
  +-- FileSearchSession (handle)
        update_query() sends WorkSignal::QueryUpdated
        Drop sends WorkSignal::Shutdown
```

### Key Types

- `FileSearchSession` -- opaque handle; the only public API is `update_query(&str)`
- `SessionReporter` trait -- callback interface with `on_update(snapshot)` and `on_complete()`
- `FileSearchSnapshot` -- contains `query`, `matches: Vec<FileMatch>`, `total_match_count`,
  `scanned_file_count`, `walk_complete`
- `FileMatch` -- `{ score: u32, path: PathBuf, match_type: File|Directory, root: PathBuf, indices: Option<Vec<u32>> }`
- `FileSearchOptions` -- `{ limit, exclude, threads, compute_indices, respect_gitignore }`

### TUI Integration

- `FileSearchManager` (in `tui/src/file_search.rs`) owns one `FileSearchSession` at a time
- `ChatComposer` detects `@token` under cursor, sends `AppEvent::StartFileSearch(query)`
- `App` calls `file_search.on_user_query(query)` which creates/updates the session
- Results arrive as `AppEvent::FileSearchResult { query, matches }` via the `SessionReporter`
- `FileSearchPopup` renders the matches with highlighted indices

### What the Existing `codex-utils-fuzzy-match` Crate Provides

The workspace already has `codex-utils-fuzzy-match` (`runtime/codex-upstream/codex-rs/utils/fuzzy-match/`),
a zero-dependency, single-threaded fuzzy matcher:

```rust
pub fn fuzzy_match(haystack: &str, needle: &str) -> Option<(Vec<usize>, i32)>
```

- Case-insensitive subsequence matching with Unicode support
- Returns match indices (for highlighting) and a score (lower = better)
- Used by `ChatComposer` for slash-command and skill popup filtering
- No threads, no external dependencies -- compiles trivially for wasm32-wasip2

## Implementation Options

### Option A: Single-Threaded Nucleo + Sequential Walk

Replace `build_parallel()` with `build()` (sequential walker) and `Nucleo::new(..., Some(1), ...)`
(single matcher thread -- actually runs on the cooperative tokio scheduler via `thread_spawn`).

**Pros**: Minimal code change; preserves nucleo's superior scoring algorithm.
**Cons**: `crossbeam-channel::select!` is a blocking call that will deadlock in cooperative
WASM. The matcher_worker's event loop fundamentally requires real thread blocking.
`ignore` crate's sequential `Walk` may work but depends on `std::fs` which routes through
WASI -- needs testing. Nucleo's internal threading (even with thread count = 1) still uses
`crossbeam` for its notify callback.

**Feasibility**: Low. The crossbeam select loop is the core blocker. Would require
rewriting the entire matcher_worker to be async, defeating the purpose.

### Option B: Replace File Search Internals with fuzzy-match + WASI fs (Rust-side)

Write a wasm32-only `create_session()` that:
1. Walks the WASI filesystem using `std::fs::read_dir` recursively (synchronous, goes
   through the OPFS shim via WASI)
2. Runs `codex-utils-fuzzy-match::fuzzy_match()` against each path
3. Returns top-N results sorted by score

**Pros**: Uses existing crates already in the workspace. Stays entirely in Rust.
Everything compiles for wasm32-wasip2. Preserves the `FileSearchSession` / `SessionReporter`
API so TUI integration code needs zero changes.

**Cons**: Sequential walk + match is O(N) per keystroke for the full file tree. For small
projects (< 10k files, typical for browser sandbox) this is fine. No incremental matching
(nucleo caches state between keystrokes). The `ignore` crate's `.gitignore` support would
be lost -- but the OPFS sandbox typically does not have `.gitignore` patterns that matter
(the workspace is already filtered by what was cloned).

**Feasibility**: High. The WASI `read_dir` path already works (the TUI's `ls` command
uses it). `fuzzy_match` is proven. Main work is writing the glue code.

### Option C: Implement File Search in JavaScript (WIT Interface)

Define a new WIT interface `file-search` that the JS host implements:
- `search(query: string, directories: list<string>, limit: u32) -> list<file-match>`
- JS side uses `listDirectory()` from `directory-tree.ts` for recursive OPFS traversal
- JS side does fuzzy matching (port or use a JS fuzzy library)

**Pros**: Non-blocking OPFS access (async JS can iterate OPFS handles directly).
Could cache the file list across searches. Full access to OPFS APIs.

**Cons**: Requires new WIT interface, new JS shim, new transpile config, new Vite path
mapping. Duplicates logic that could live in Rust. The Rust `FileSearchSession` API
would need a bridge layer. Most complex option.

**Feasibility**: Medium. More moving parts than Option B.

### Option D: Hybrid -- Rust Fuzzy Match + JS File List via WIT

Define a minimal WIT interface that only provides the file listing:
- `list-all-files(root: string) -> list<string>` (returns all file paths recursively)
- Rust side receives the list, runs `fuzzy_match` on each entry
- Avoids WASI `read_dir` (which can be slow for deep OPFS trees) by using native OPFS
  iteration on the JS side

**Pros**: Clean separation -- JS handles async I/O, Rust handles matching.
File list can be cached and invalidated on filesystem changes.

**Cons**: Still requires a new WIT interface. The WASI `read_dir` path already works,
so the JS optimization may be premature.

**Feasibility**: Medium. Good architecture but may be over-engineered for v1.

## Recommended Approach: Option B (Rust-side, fuzzy-match + WASI fs)

Option B is the right choice for a first implementation:

1. **Minimal new code**: ~150-200 lines of Rust in `file-search/src/lib.rs`
2. **Zero new dependencies**: Uses `codex-utils-fuzzy-match` (already in workspace) and
   `std::fs` (already shimmed by WASI)
3. **Zero TUI changes**: Preserves `FileSearchSession` / `SessionReporter` API
4. **Zero JS changes**: No new WIT interfaces, shims, or Vite config
5. **Performance is adequate**: Browser sandboxes typically have < 5k files. A linear
   scan with fuzzy match takes < 50ms for that scale.

If performance becomes a problem (large projects), Option D can be layered on later
by adding a cached file list from the JS side.

## Concrete Implementation Steps

### Step 1: Add `codex-utils-fuzzy-match` dependency to `file-search`

**File**: `runtime/codex-upstream/codex-rs/file-search/Cargo.toml`

Add under `[dependencies]`:
```toml
codex-utils-fuzzy-match = { path = "../utils/fuzzy-match" }
```

This will be picked up by the codemod since `file-search` is already a workspace member.

### Step 2: Write wasm32 implementation in `file-search/src/lib.rs`

**File**: `runtime/codex-upstream/codex-rs/file-search/src/lib.rs`

Replace the `#[cfg(target_arch = "wasm32")]` bail in `create_session()` with a real
implementation. The wasm32 path should:

```rust
#[cfg(target_arch = "wasm32")]
{
    return create_session_wasm(search_directories, options, reporter);
}
```

New function `create_session_wasm()`:

1. **Walk**: Use `std::fs::read_dir` recursively to collect all file paths under
   each search directory. Skip hidden directories (`.git`, `node_modules`) for
   performance. Respect `options.exclude` patterns.

2. **Store file list**: Keep the collected paths in the session state (wrapped in
   `Arc<Mutex<Vec<String>>>`).

3. **Match on query update**: When `update_query()` is called, run
   `codex_utils_fuzzy_match::fuzzy_match(path, query)` on each path. Collect
   matches, sort by score, take top `options.limit`, convert to `FileMatch` structs.

4. **Report**: Call `reporter.on_update()` with the snapshot, then `reporter.on_complete()`.

5. **Session lifecycle**: The `FileSearchSession` holds the file list. `update_query()`
   re-runs matching synchronously (fast for < 10k files). `Drop` is a no-op.

The key simplification: no background threads, no channels, no incremental matching.
Everything runs synchronously in `update_query()`.

### Step 3: Update the codemod to inject the wasm32 implementation

**File**: `runtime/codex-wasm/codex-codemod/src/syn_transforms.rs`

Change the existing codemod entry that injects the bail. Instead of bailing, inject the
`create_session_wasm` function and the `#[cfg(target_arch = "wasm32")]` early-return in
`create_session()`.

Alternatively, add the wasm32 code directly to the upstream `file-search/src/lib.rs`
behind `#[cfg(target_arch = "wasm32")]` -- this is cleaner if the upstream accepts it,
but the codemod approach keeps the upstream pristine.

### Step 4: Handle score conversion

The `codex-utils-fuzzy-match` crate returns `i32` scores (lower = better).
`FileMatch.score` is `u32` (higher = better, from nucleo).
The TUI sorts matches by descending score.

Convert: `u32_score = (i32::MAX - fuzzy_score) as u32`

This preserves relative ordering when the TUI sorts by descending `score`.

### Step 5: Handle match indices conversion

`codex-utils-fuzzy-match::fuzzy_match` returns `Vec<usize>` indices.
`FileMatch.indices` expects `Option<Vec<u32>>`.

Convert: `indices.iter().map(|&i| i as u32).collect()`

This is needed for highlighting in the popup.

### Step 6: Test

- Verify `@` popup appears and shows files from the OPFS sandbox
- Verify fuzzy matching works (typing `main` matches `src/main.rs`)
- Verify highlighted characters are correct
- Verify selecting a match inserts the path into the prompt
- Verify performance is acceptable with a typical cloned repository

## File Summary

| File | Action |
|------|--------|
| `runtime/codex-upstream/codex-rs/file-search/Cargo.toml` | Add `codex-utils-fuzzy-match` dep |
| `runtime/codex-upstream/codex-rs/file-search/src/lib.rs` | Add `create_session_wasm()` behind `#[cfg(wasm32)]` |
| `runtime/codex-wasm/codex-codemod/src/syn_transforms.rs` | Update codemod: replace bail with early-return to wasm impl |
| `runtime/codex-wasm/codex-codemod/src/cargo_toml.rs` | No change needed (file-search already a workspace member) |

No changes needed in:
- TUI code (`file_search.rs`, `file_search_popup.rs`, `chat_composer.rs`, `app.rs`)
- JS shims (`opfs-filesystem-impl.ts`, `directory-tree.ts`)
- WIT interfaces (`runtime/wit/`)
- Transpile config (`scripts/transpile.mjs`)
- Vite config (`frontend/vite.config.ts`)

## Future Enhancements

1. **Cached file list**: Walk once on session creation, re-walk only on filesystem changes.
   The current design re-walks on every `create_session` call but reuses the list across
   `update_query` calls within a session -- this is already how the upstream works.

2. **Gitignore support**: Parse `.gitignore` files from the OPFS sandbox and filter
   the walk results. Low priority since browser sandboxes are typically clean clones.

3. **JS-backed file list (Option D)**: If WASI `read_dir` proves too slow for large
   projects, add a WIT interface to get the file list from JS (which has direct OPFS
   handle iteration, avoiding the WASI overhead).

4. **Incremental matching**: Cache match results and only re-score when the query
   changes in a way that could change results (e.g., appending narrows, deleting widens).
   This is what nucleo does natively.
