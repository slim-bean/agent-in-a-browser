# Git Implementation in WASM/OPFS Environment

## Current State

### Two Separate Git Paths

There are two distinct layers where git operations occur in this codebase, and they currently use completely different mechanisms:

#### 1. Codex TUI (upstream codex-rs) -- shell-exec via MCP

The codex-rs core (`runtime/codex-upstream/codex-rs/core/src/git_info.rs`) runs git commands by calling `tokio::process::Command::new("git")`. In WASM, the custom `wasi-tokio` shim (`runtime/codex-wasm/wasi-tokio/src/process.rs`) intercepts all `Command` calls and routes them through a `ProcessBackend` trait. The codex-wasm-tui registers a `WasiShellBackend` (`runtime/codex-wasm/codex-wasm-tui/src/shell_exec_backend.rs`) that calls into the WIT `shell-exec` interface.

The WIT `shell-exec` interface (`runtime/codex-wasm/codex-wasm-tui/wit/world.wit`) is implemented by the JS host (wasm-worker.ts, lines 593-657). The host serializes the command as a `tools/call` JSON-RPC request to `run_command` and sends it to the MCP server via HTTP through the SharedWorker.

**Flow:** Rust `Command::new("git")` -> wasi-tokio ProcessBackend -> WIT shell-exec -> JS wasm-worker.ts -> postMessage to main thread -> SharedWorker MCP -> ts-runtime-mcp `run_command` tool -> ts-runtime-mcp shell executor -> lazy module loader -> git-module (isomorphic-git)

This is the path taken when the upstream codex-rs code calls functions like:
- `collect_git_info()` -- runs `git rev-parse --git-dir`, `git rev-parse HEAD`, `git rev-parse --abbrev-ref HEAD`, `git remote get-url origin`
- `git_diff_to_remote()` -- runs `git diff`, `git rev-list`, `git for-each-ref`, `git remote`, `git symbolic-ref`, etc.
- `merge_base_with_head()` -- runs `git merge-base`, `git rev-parse --verify`, etc.
- `recent_commits()` -- runs `git log --pretty=format:...`
- Ghost commit operations (create/restore) -- runs `git add`, `git commit-tree`, `git update-ref`, `git status`, `git ls-files`, etc.

#### 2. ts-runtime-mcp Interactive Shell -- isomorphic-git directly

The WASM MCP server (`runtime/src/`) has a built-in shell executor that handles commands locally. Git commands are registered as a lazy-loadable module (`runtime/src/shell/commands/git.rs`). The shell executor (`runtime/src/shell/new_executor.rs`, line 846-878) checks `loader::get_lazy_module("git")` which returns `"git-module"`. The frontend's lazy module system (`frontend/src/wasm/lazy-loading/lazy-modules.ts`) maps this to `frontend/src/wasm/git/git-module.ts`, which uses isomorphic-git directly against OPFS.

**Flow:** ts-runtime-mcp shell -> lazy module loader -> git-module.ts -> isomorphic-git -> opfs-git-adapter.ts -> OPFS

### What isomorphic-git Currently Supports

The git-module (`frontend/src/wasm/git/git-module.ts`) implements:

| Subcommand | Async (JSPI) | Sync (Safari) | Notes |
|-----------|-------------|--------------|-------|
| `init` | Yes | Yes | Full implementation |
| `clone` | Yes | No | Uses CORS proxy, supports `--depth`, `--single-branch` |
| `status` | Yes | Partial | Sync mode: hardcoded "No commits yet" |
| `add` | Yes | No | |
| `commit` | Yes | No | Hardcoded author "Web Agent" |
| `log` | Yes | No | Supports `-n N` |
| `branch` | Yes | No | List and create |
| `checkout` | Yes | No | |

**Missing subcommands:** `rev-parse`, `diff`, `merge-base`, `remote`, `ls-files`, `rev-list`, `for-each-ref`, `symbolic-ref`, `commit-tree`, `update-ref`, `apply`, `fetch`, `push`, `stash`, `reset`, `rm`.

### The OPFS-Git Adapter

`frontend/src/wasm/git/opfs-git-adapter.ts` implements a Node.js `fs`-compatible API backed by OPFS. It provides: `readFile`, `writeFile`, `unlink`, `readdir`, `mkdir`, `rmdir`, `stat`, `lstat`, `readlink`, `symlink`, `chmod`, `rename`. Symlinks are not supported (OPFS limitation) -- `readlink` throws, `symlink` silently ignores.

### The Problem

When the codex-rs core (path 1) runs `git rev-parse --git-dir`, this goes through a 6-hop round trip:

```
Rust WASM -> WIT -> JS Worker -> postMessage -> SharedWorker -> MCP HTTP -> ts-runtime-mcp shell -> lazy module -> git-module -> isomorphic-git
```

At the ts-runtime-mcp shell level, the command is parsed as a shell string. But the git-module only handles a subset of subcommands. For `rev-parse --git-dir`, the git-module returns `git: 'rev-parse' is not a git command.` (exit code 1). This means:

- `collect_git_info()` fails immediately (thinks we're not in a git repo)
- `git_diff_to_remote()` fails
- Ghost commits fail
- `merge_base_with_head()` fails

The upstream codex-rs uses **dozens** of distinct git subcommands and flag combinations. The isomorphic-git adapter only handles 8 basic subcommands.

## What Codex-RS Actually Needs from Git

### git_info.rs (context gathering, called every turn)

```
git rev-parse --git-dir
git rev-parse HEAD
git rev-parse --abbrev-ref HEAD
git remote get-url origin
git remote
git remote -v
git remote show <remote>
git symbolic-ref --quiet refs/remotes/<remote>/HEAD
git rev-parse --verify --quiet refs/heads/<branch>
git for-each-ref --format=%(refname:short) --contains=HEAD refs/remotes/<remote>
git rev-list --count <branch>..HEAD
git rev-parse --verify <branch>
git diff --no-textconv --no-ext-diff <sha>
git ls-files --others --exclude-standard
git diff --no-textconv --no-ext-diff --binary --no-index -- /dev/null <file>
git status --porcelain
git log -n <N> --pretty=format:%H%x1f%ct%x1f%s
git branch --format=%(refname:short)
git branch --show-current
```

### operations.rs + branch.rs (used by ghost commits and merge-base)

```
git rev-parse --is-inside-work-tree
git rev-parse --show-toplevel
git merge-base <sha1> <sha2>
git rev-list --left-right --count <branch>...<upstream>
```

### ghost_commits.rs (undo/snapshot)

```
git status -z --porcelain
git ls-files -z --others --exclude-standard
git ls-files -z --others --no-exclude-standard
git add -A --force -- <paths>
git write-tree
git commit-tree <tree> [-p <parent>] -m <message>
git update-ref refs/codex/<name> <sha>
git read-tree <sha>
git checkout-index --all --force
git symbolic-ref HEAD
git reset --hard <sha>
git clean -fd
```

### apply.rs (patch application)

```
git apply --3way [--check] [-R] <patchfile>
git add <paths>
git rev-parse --show-toplevel
```

## Architecture Options

### Option A: Expand isomorphic-git CLI Shim (Recommended)

**Approach:** Add `rev-parse`, `diff`, `remote`, `ls-files`, `rev-list`, etc. to `git-module.ts` using isomorphic-git's API.

**Feasibility per category:**

| Operation | isomorphic-git API | Difficulty |
|----------|-------------------|-----------|
| `rev-parse HEAD` | `git.resolveRef({ref: 'HEAD'})` | Easy |
| `rev-parse --git-dir` | Check `.git` dir exists in OPFS | Easy |
| `rev-parse --abbrev-ref HEAD` | `git.currentBranch()` | Easy |
| `rev-parse --is-inside-work-tree` | Check `.git` dir exists | Easy |
| `rev-parse --show-toplevel` | Walk up looking for `.git` | Easy |
| `rev-parse --verify <ref>` | `git.resolveRef({ref})` | Easy |
| `status --porcelain` | `git.statusMatrix()` -> format | Medium |
| `status -z --porcelain` | Same, NUL-delimited | Medium |
| `log --pretty=format:...` | `git.log()` -> format | Medium |
| `diff <sha>` | `git.walk()` + content compare | Hard |
| `diff --no-index` | Direct file compare | Medium |
| `branch --format=...` | `git.listBranches()` -> format | Easy |
| `branch --show-current` | `git.currentBranch()` | Easy |
| `remote` / `remote -v` | `git.listRemotes()` | Easy |
| `remote get-url origin` | `git.getConfig({path: 'remote.origin.url'})` | Easy |
| `remote show <remote>` | Partial: config + fetch | Hard |
| `merge-base` | Not in isomorphic-git directly | Hard |
| `ls-files --others --exclude-standard` | `git.statusMatrix()` -> filter | Medium |
| `for-each-ref --contains=HEAD` | `git.listBranches()` + `git.log()` | Hard |
| `rev-list --count` | `git.log()` with length | Medium |
| `symbolic-ref` | `git.resolveRef()` + check type | Medium |
| `add -A --force` | `git.add()` loop | Medium |
| `write-tree` | Not directly exposed | Hard |
| `commit-tree` | Not directly exposed | Hard |
| `update-ref` | Not directly exposed | Hard |
| `read-tree` | Not directly exposed | Hard |
| `checkout-index` | Not directly exposed | Hard |
| `apply --3way` | Not supported | Not feasible |
| `clone` / `fetch` / `push` | `git.clone/fetch/push()` with `http` transport | Already working |

**Pros:**
- Reuses existing pattern (lazy module, OPFS adapter already working)
- isomorphic-git handles the hard parts (packfile parsing, index management, object store)
- No additional WASM compilation needed
- Clone/fetch/push work via HTTP transport + `fetch()` API
- Already in `package.json`, already integrated

**Cons:**
- isomorphic-git does not expose low-level plumbing commands (`write-tree`, `commit-tree`, `update-ref`, `read-tree`, `checkout-index`)
- `git apply --3way` is not supported at all
- `merge-base` requires manual graph traversal
- Some operations need creative workarounds
- CLI flag parsing must be done manually for every subcommand

**Verdict:** This is the right path for **Phase 1** (context gathering). The context-gathering operations in `git_info.rs` are all achievable. Ghost commits and patch application need a different approach.

### Option B: WIT Interface for Git Operations

**Approach:** Define a `git-operations` WIT interface with structured operations (not CLI-shaped), backed by isomorphic-git on the host side.

```wit
interface git-operations {
    resolve-ref: func(dir: string, ref: string) -> result<string, string>;
    current-branch: func(dir: string) -> result<option<string>, string>;
    status-matrix: func(dir: string) -> result<list<file-status>, string>;
    list-remotes: func(dir: string) -> result<list<remote-info>, string>;
    log: func(dir: string, depth: u32) -> result<list<commit-info>, string>;
    diff-trees: func(dir: string, sha1: string, sha2: string) -> result<string, string>;
    // ...
}
```

**Pros:**
- Type-safe, no CLI parsing
- Can be optimized (batch operations, avoid serialization overhead)
- Clean separation of concerns

**Cons:**
- Requires modifying the codex-rs core to use this interface instead of `Command::new("git")` -- massive upstream divergence
- Every new git operation needs WIT changes + Rust bindings + JS implementation
- Does not help with the upstream `codex-git` crate which shells out to `git`

**Verdict:** Too much upstream divergence. The codex-rs code already has a clean `ProcessBackend` abstraction. Better to make that work well.

### Option C: Compile libgit2 to WASM

**Approach:** Compile libgit2 (or gitoxide/gix) to wasm32-wasip2, link it into codex-wasm-tui.

**Pros:**
- Full git compatibility, every plumbing command works
- No JS interop for git operations
- gitoxide (gix) is pure Rust, should compile to WASM

**Cons:**
- libgit2 has C dependencies (zlib, OpenSSL for HTTPS, etc.) -- complex cross-compilation
- gitoxide is large, adds significant WASM binary size
- Network operations (clone/fetch/push) need HTTP transport shim
- File locking (`git index.lock`) has OPFS constraints
- Significant build complexity

**Verdict:** Worth investigating for Phase 3 if gitoxide's subset compilation works. The `gix` crate has a modular design where you can pull in just what you need.

### Option D: Status Quo (MCP run_command Round-Trip)

**Approach:** Keep routing all git commands through the MCP `run_command` path, but make the git-module handle more subcommands.

This is actually what Option A is, since the MCP `run_command` already routes to the git-module. The difference is that currently the git-module rejects most subcommands.

**Verdict:** This IS Option A -- expanding the git-module to handle the subcommands that codex-rs actually issues.

## Recommended Plan

### Phase 1: Make Context Gathering Work (High Priority)

The immediate blocker is `collect_git_info()` failing because `rev-parse --git-dir` is not handled. This causes every user turn to lack git context.

**Implementation:** Add these subcommands to `git-module.ts`:

1. **`rev-parse`** -- the critical one
   - `--git-dir`: Check if `.git` exists in cwd (or walk up), return `.git` path
   - `HEAD`: `git.resolveRef({ref: 'HEAD'})`
   - `--abbrev-ref HEAD`: `git.currentBranch()`
   - `--verify <ref>`: `git.resolveRef({ref})`
   - `--is-inside-work-tree`: Check `.git` existence, return "true"
   - `--show-toplevel`: Walk up from cwd to find `.git`, return that directory

2. **`remote`** (no args): `git.listRemotes()`, print names
3. **`remote -v`**: `git.listRemotes()`, print with URLs
4. **`remote get-url <name>`**: `git.getConfig({path: 'remote.<name>.url'})`
5. **`status --porcelain`**: `git.statusMatrix()`, format as porcelain output
6. **`log --pretty=format:...`**: `git.log()`, format output per format string
7. **`branch --format=...`**: `git.listBranches()`, format output
8. **`branch --show-current`**: `git.currentBranch()`
9. **`diff <sha>`**: Use `git.walk()` to compare trees (basic implementation)
10. **`ls-files --others --exclude-standard`**: `git.statusMatrix()`, filter untracked
11. **`symbolic-ref`**: `git.resolveRef()` on the symbolic ref path

**Estimated effort:** 2-3 days. The isomorphic-git APIs map cleanly to most of these.

### Phase 2: Initialize Repository on Workspace Setup

Currently there is no `.git` directory in the OPFS workspace, so `rev-parse --git-dir` will always fail even with the Phase 1 implementation.

**Options:**
1. **Auto-init on workspace creation:** When the WASM worker boots and creates `/workspace`, also run `git init` via isomorphic-git. This gives codex-rs a valid git repo to work with.
2. **Virtual .git stub:** Create a minimal `.git/HEAD` and `.git/config` in OPFS without a full init. Enough for `rev-parse` to succeed.
3. **Null git context:** Modify the codex-rs codemod to make `collect_git_info()` return a synthetic response in WASM. Least disruptive but loses real git functionality.

**Recommendation:** Option 1 (auto-init). It is simple, takes one line (`await git.init({fs, dir: '/workspace'})`), and gives a real repo that can track changes.

### Phase 3: Ghost Commits / Undo (Lower Priority)

Ghost commits use low-level plumbing (`write-tree`, `commit-tree`, `update-ref`, `read-tree`, `checkout-index`). isomorphic-git does not expose these.

**Options:**
1. **Reimplement in JS:** Use isomorphic-git's internal APIs or raw object manipulation. isomorphic-git stores objects in the same format as git, so we can read/write them directly.
2. **Disable ghost commits in WASM:** The codex-codemod already strips/modifies upstream code. Ghost commits could be no-op'd.
3. **Use gitoxide in WASM (Phase 3):** If gix compiles cleanly to wasm32-wasip2, it would provide all plumbing commands natively.

**Recommendation:** Option 2 for now (disable in WASM), pursue Option 1 later if undo functionality is needed.

### Phase 4: Clone / Fetch / Push from OPFS

isomorphic-git already supports clone/fetch/push via its HTTP transport. The existing `git clone` implementation in git-module.ts works with a CORS proxy. For authenticated operations:

- **GitHub:** Use OAuth token from the existing OAuth flow (bridge/oauth_client.rs)
- **Transport:** isomorphic-git's `http/web` module uses `fetch()`, which works in Workers
- **CORS:** GitHub's git HTTP protocol doesn't support CORS. Options:
  - CORS proxy (current approach, privacy concern)
  - Service worker proxy (intercept and add CORS headers)
  - GitHub API for tree/blob operations (different protocol, better CORS)

## OPFS as Git Backing Store

### What Works

- **Object store (`.git/objects/`):** Binary blobs, fine in OPFS
- **Refs (`.git/refs/`):** Small text files, fine in OPFS
- **Index (`.git/index`):** Binary file, fine in OPFS
- **Config (`.git/config`):** Text file, fine in OPFS
- **Working tree:** Regular files in OPFS, fine
- **Packfiles (`.git/objects/pack/`):** Large binary files, OPFS handles them (with performance caveats for very large repos)

### Limitations

- **No symlinks:** OPFS does not support symlinks. Repos that use symlinks will have issues. isomorphic-git's adapter already handles this by silently ignoring `symlink()` calls.
- **No file permissions:** OPFS does not track Unix permissions. `git status` may show false modifications for repos that track executable bits.
- **No file locking:** OPFS does not have `flock()`-style locks. Concurrent git operations could corrupt the index. In practice, single-threaded WASM execution prevents this.
- **Performance:** OPFS operations are async (or sync via `createSyncAccessHandle` in Workers). Large repos with many objects will be slower than native git.
- **Storage quota:** OPFS is subject to browser storage quotas (~10% of disk on Chrome). Large repos may hit this limit.
- **No hardlinks:** Git occasionally uses hardlinks for efficiency. OPFS does not support them.

### Storage Persistence

OPFS data persists across browser sessions but can be evicted under storage pressure. For non-disposable repos:
- Consider periodic snapshots
- Warn users that OPFS storage is not permanent
- Support re-cloning from remote

## Summary Table

| Priority | What | Approach | Effort |
|----------|------|----------|--------|
| P0 | `rev-parse` + context gathering | Expand git-module.ts | 2-3 days |
| P0 | Auto-init workspace as git repo | `git.init()` on boot | 1 hour |
| P1 | `status --porcelain`, `log --pretty` | Expand git-module.ts | 1-2 days |
| P1 | `diff`, `ls-files`, `remote` | Expand git-module.ts | 2-3 days |
| P2 | Ghost commit / undo | Disable or reimplement in JS | 1-2 weeks |
| P2 | `merge-base` | Manual graph traversal | 2-3 days |
| P3 | `git apply` | Not feasible with isomorphic-git | Needs libgit2/gix |
| P3 | Authenticated clone/push | OAuth integration | 1 week |
| P3 | gitoxide in WASM | Investigate gix compilation | Unknown |
