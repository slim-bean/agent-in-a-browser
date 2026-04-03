# Git Implementation in WASM/OPFS Environment

## Current Architecture

Git is implemented as a standalone Go CLI (`git-cli-wasm/`) using [go-git](https://github.com/go-git/go-git) (pure Go, no C dependencies), compiled to `GOOS=wasip1 GOARCH=wasm`. It follows the same wasip1 bridge pattern as the stripe-cli module.

### Build Pipeline

```
go build (wasip1) → git.wasm
  → wasm-tools component embed (WIT metadata)
  → wasm-tools component new (wasip1→wasip2 adapter)
  → git-component.wasm → /target/wasm32-wasip2/release/git_go.wasm
```

Moon tasks: `git-cli-wasm:build-go-wasm` → `adapt-component` → `copy-to-target`

### Runtime Loading

The frontend lazy-loads the git module on first use via the direct wasip1 loader (bypasses JCO component model to avoid stack overflow from Go's init functions):

```
Shell command "git ..." → lazy module loader → loadGitModule()
  → go-wasip1-loader.ts (direct WASM instantiation)
  → OPFS filesystem (via WASI Preview1)
  → HTTP bridge (for clone/fetch/push over network)
```

### Data Flow

```
ts-runtime-mcp shell → get_lazy_module("git") → "git-module"
  → frontend lazy-modules.ts → loadGitModule()
    → go-wasip1-loader.ts (WebAssembly.instantiate with WASI P1 imports)
      ├── OPFS filesystem (file I/O via SyncAccessHandle / async fallback)
      ├── git:bridge/http-bridge (network ops → browser fetch() API)
      └── stdout/stderr → shell output
```

For codex-rs TUI, the path is longer:
```
Rust Command::new("git") → wasi-tokio ProcessBackend → WIT shell-exec
  → JS wasm-worker.ts → SharedWorker MCP → ts-runtime-mcp run_command
  → shell executor → lazy module → git-module (go-git WASM)
```

### HTTP Bridge

The Go binary uses `//go:wasmimport git:bridge/http-bridge@0.1.0` to route HTTP requests through the browser's `fetch()` API. The bridge is defined in:

- **Go side:** `git-cli-wasm/internal/transport/bridge.go` — implements `http.RoundTripper`, registers with go-git's transport layer
- **WIT contract:** `git-cli-wasm/wit/` — defines `git:bridge/http-bridge@0.1.0`
- **JS shim:** `packages/wasi-shims/src/http-bridge-impl.ts` — bridges to `fetch()` with CORS proxy support

The same `http-bridge-impl.ts` shim is shared with the stripe-cli module.

### go-git Patches

The go-git dependency uses a patched fork (`git-cli-wasm/patches/go-git/`, submodule at `tjfontaine/go-git` branch `wasip1-patches`) that stubs out platform-specific syscalls unavailable in WASI (e.g., `syscall.Stat_t` fields for worktree metadata).

## Supported Commands

| Subcommand | Status | Notes |
|-----------|--------|-------|
| `init` | Working | |
| `clone` | Working | Via HTTP bridge + CORS proxy |
| `status` | Working | |
| `add` | Working | Supports `.` and `--all` |
| `commit` | Working | |
| `log` | Working | Supports `-n N` |
| `diff` | Working | Worktree and cached |
| `branch` | Working | List and create |
| `checkout` | Working | Switch branches |
| `fetch` | Working | |
| `pull` | Working | |
| `push` | Working | |
| `remote` | Working | Manage remotes |
| `tag` | Working | |

### Missing for Full Codex-RS Support

The codex-rs core uses plumbing commands that go-git doesn't expose as CLI subcommands yet:

| Operation | Used By | Difficulty |
|----------|---------|-----------|
| `rev-parse --git-dir`, `HEAD`, `--abbrev-ref` | `collect_git_info()` | Medium — needs new subcommand |
| `rev-parse --is-inside-work-tree`, `--show-toplevel` | `operations.rs` | Medium |
| `symbolic-ref` | ghost commits, branch detection | Medium |
| `merge-base` | `merge_base_with_head()` | Medium — go-git has the API |
| `ls-files --others --exclude-standard` | ghost commits | Medium |
| `for-each-ref --contains=HEAD` | branch detection | Hard |
| `rev-list --count` | ahead/behind calculation | Medium |
| `write-tree`, `commit-tree`, `update-ref` | ghost commits | Hard — low-level plumbing |
| `read-tree`, `checkout-index` | ghost commit restore | Hard |
| `apply --3way` | patch application | Hard |

## OPFS as Git Backing Store

### What Works

- **Object store** (`.git/objects/`): Binary blobs, fine in OPFS
- **Refs** (`.git/refs/`): Small text files, fine in OPFS
- **Index** (`.git/index`): Binary file, fine in OPFS
- **Config** (`.git/config`): Text file, fine in OPFS
- **Working tree**: Regular files in OPFS
- **Packfiles** (`.git/objects/pack/`): Large binary files (performance caveats for very large repos)

### Limitations

- **No symlinks:** OPFS does not support symlinks
- **No file permissions:** OPFS does not track Unix permissions; `git status` may show false modifications
- **No file locking:** No `flock()`-style locks; single-threaded WASM execution prevents concurrent corruption
- **Storage quota:** Subject to browser storage quotas (~10% of disk on Chrome)

## Key Files

| File | Purpose |
|------|---------|
| `git-cli-wasm/cmd/git/main.go` | CLI entry point |
| `git-cli-wasm/internal/commands/` | Subcommand implementations |
| `git-cli-wasm/internal/transport/bridge.go` | HTTP bridge (`//go:wasmimport`) |
| `git-cli-wasm/wit/` | WIT interface definitions |
| `git-cli-wasm/patches/go-git/` | Patched go-git for WASI |
| `git-cli-wasm/moon.yml` | Build pipeline |
| `packages/wasm-git/` | npm package metadata + transpiled output |
| `packages/wasi-shims/src/http-bridge-impl.ts` | JS HTTP bridge shim |
| `frontend/src/wasm/lazy-loading/lazy-modules.ts` | `loadGitModule()` |
| `frontend/src/wasm/lazy-loading/go-wasip1-loader.ts` | Direct wasip1 WASM loader |
| `runtime/src/shell/commands/git.rs` | Shell command registration stub |
