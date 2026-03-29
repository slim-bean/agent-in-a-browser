# Zip Extraction Support in Codex TUI WASM Build

## Status

**Not started.** This document was last validated against the codebase on 2026-03-28.

## Problem

Remote skill/plugin downloads (`export_remote_skill` in `core/src/skills/remote.rs`)
require zip extraction. The `zip` crate (zip-rs v2.4.2) was stripped from the WASM
build because its default features pull in `zstd` (C code via `zstd-sys`), which
cannot compile for `wasm32-wasip2`. The codemod replaces the function body with a
bail stub:

```rust
fn extract_zip_to_dir(bytes: Vec<u8>, output_dir: &Path, prefix_candidates: &[String]) -> Result<()> {
    let _ = (bytes, output_dir, prefix_candidates);
    anyhow::bail!("zip extraction not available in WASM")
}
```

The `package-manager/src/archive.rs` module is also fully stubbed (whole-file
replacement in `ast_transforms.rs`) for the same reason. `package-manager` IS in the
`KEEP_WORKSPACE_MEMBERS` allowlist, so it is compiled.

## How Things Work Today

The codemod runs **offline** (not at build time). The submodule at
`runtime/codex-upstream` carries pre-applied codemod output. After rebasing upstream,
the developer runs `cargo run -p codex-codemod -- codex-upstream` and commits the
result in the submodule fork.

The zip-related codemod lives in **two places** (both must be updated):

1. **`syn_transforms.rs` line ~1388** -- `collect_string_replacement_edits()` calls
   `self.string_replace("core/src/skills/remote.rs", ...)` to replace the
   `extract_zip_to_dir` body with the bail stub. This is the **active** code path
   used when the codemod runs.

2. **`ast_transforms.rs` line ~110** -- The legacy `STRING_REPLACEMENTS` array
   contains the same entry. This array is still applied by `transform_file()` at
   line ~2675. Both run on the same file; the second one silently skips if the
   replacement text is already present (see the `else if !modified.contains(replace)`
   guard). Both must be removed to avoid stale-pattern warnings on future upstream
   changes.

3. **`ast_transforms.rs` line ~2390** -- `FULL_FILE_REPLACEMENTS` has a whole-file
   stub for `package-manager/src/archive.rs`.

4. **`cargo_toml.rs` line 177-178** -- `"zip"` in `STRIP_DEPENDENCIES`.
   Line 185 -- `"flate2"` in `STRIP_DEPENDENCIES`.

## Analysis: Can zip-rs Compile for wasm32-wasip2?

**Yes, with the right feature flags.**

zip-rs v2.4.2 default features include: `aes-crypto`, `bzip2`, `deflate64`,
`deflate`, `lzma`, `ppmd`, `time`, `zstd`, `xz`. The problematic ones:

| Feature  | Underlying crate       | C code? | WASM-safe? |
|----------|------------------------|---------|------------|
| `zstd`   | `zstd` (zstd-sys)      | Yes     | No         |
| `bzip2`  | `bzip2` (bzip2-sys)    | Yes     | No         |
| `deflate`| `flate2` (miniz_oxide) | No      | Yes        |
| `deflate64` | `deflate64`         | No      | Yes        |
| `lzma`   | `lzma-rust2`           | No      | Yes        |
| `xz`     | `lzma-rust2`           | No      | Yes        |
| `aes-crypto` | `aes`, `hmac`, etc | No      | Yes        |
| `time`   | `time`                 | No      | Yes        |

Most real-world zip files use deflate.

## Architecture Decision: No Shim Crate Needed

The `wasi-sqlx` pattern (a full shim crate that re-implements the API) is
**overkill** for zip. The `sqlx` crate needed a shim because it depends on `ring`
(C code with no feature-flag escape hatch) and its entire API needed replacement
with `rusqlite`. In contrast, zip-rs is pure Rust when you disable `zstd` and
`bzip2` -- the upstream code works as-is.

The correct approach:
1. **Un-strip** `zip` and `flate2` from the codemod's `STRIP_DEPENDENCIES`.
2. **Override features** in the consumer `Cargo.toml` files to exclude C-dependent
   features.
3. **Remove** the bail stubs from the codemod.
4. **Re-run** the codemod and rebuild.

No `[patch.crates-io]` entry for zip is needed. No shim crate is needed.

## Implementation Plan

### Step 1: Un-strip `zip` and `flate2` in `cargo_toml.rs`

**File:** `runtime/codex-wasm/codex-codemod/src/cargo_toml.rs`

Remove `"zip"` (line 178) and `"flate2"` (line 185) from `STRIP_DEPENDENCIES`.
Update comments:

```rust
// Before:
    // zstd — C library, doesn't compile for wasm32-wasip2
    "zstd",
    // zip — depends on zstd, artifact handling done by host
    "zip",
    // which — uses unstable wasip2 std::os::wasi feature
    "which",
    // tree-sitter — C code, doesn't compile for wasm32-wasip2
    "tree-sitter",
    "tree-sitter-bash",
    // flate2/tar — archive deps (archive handling done by host)
    "flate2",
    "tar",

// After:
    // zstd — C library, doesn't compile for wasm32-wasip2
    "zstd",
    // which — uses unstable wasip2 std::os::wasi feature
    "which",
    // tree-sitter — C code, doesn't compile for wasm32-wasip2
    "tree-sitter",
    "tree-sitter-bash",
    // tar — archive dep (tar.gz handling done by host; flate2 kept for zip)
    "tar",
```

### Step 2: Add zip feature override to consumer Cargo.toml files

Upstream declares `zip = { workspace = true }` (default features) in both
`core/Cargo.toml` and `package-manager/Cargo.toml`. The workspace root declares
`zip = "2.4.2"` (default features). Since the codemod strips the workspace root
`[workspace.dependencies]` and rewrites member deps, we need to control what
features end up in the final dependency after codemod.

**Option A (simpler): Add a codemod rule to rewrite zip deps.**

Add a new constant and logic in `cargo_toml.rs` to rewrite any `zip` dependency
to `default-features = false` with safe features. This is analogous to how
`SHIM_REDIRECTS` rewrites dependency paths, but for features instead.

Add to `cargo_toml.rs`:

```rust
/// Dependencies whose features must be overridden for WASM safety.
/// Each entry: (crate_name, &[(feature_name, ...)]).
/// These deps get `default-features = false` plus the listed features.
const REWRITE_DEPENDENCY_FEATURES: &[(&str, &[&str])] = &[
    ("zip", &["deflate", "deflate64", "lzma", "xz", "aes-crypto", "time"]),
];
```

Then in the function that processes member `Cargo.toml` files, when a dep matches
a name in `REWRITE_DEPENDENCY_FEATURES`, rewrite it to an inline table with
`version` preserved, `default-features = false`, and the specified features array.

**Option B (even simpler): Override in consumer Cargo.toml only.**

Add `zip` as a direct dependency in `codex-wasm-tui/Cargo.toml` and
`codex-wasm-agent/Cargo.toml` with restricted features:

```toml
zip = { version = "2.4", default-features = false, features = [
    "deflate", "deflate64", "lzma", "xz", "aes-crypto", "time",
] }
```

Cargo unifies features across the dependency tree. Since the consumer crate is
the root of its workspace (both are `[workspace]` standalone crates), its feature
specification controls the entire tree. If upstream's `core/Cargo.toml` still says
`zip = { workspace = true }` with default features, Cargo would unify and enable
defaults -- but the codemod rewrites workspace deps to path deps, so the workspace
`[workspace.dependencies]` is gone. The codemod would need to ensure the rewritten
`zip` dep in `core/Cargo.toml` also uses `default-features = false`.

**Recommendation: Option A.** It is more durable -- it catches zip wherever it
appears in the upstream tree automatically, and it follows the existing codemod
pattern. Option B would break whenever upstream adds zip to a new crate.

### Step 3: Remove the `extract_zip_to_dir` bail stubs

Three places to update:

**File 1:** `runtime/codex-wasm/codex-codemod/src/syn_transforms.rs` (~line 1388)

Remove this entire block:
```rust
        // --- core/src/skills/remote.rs: stub zip extraction ---
        self.string_replace(
            "core/src/skills/remote.rs",
            "    let cursor = std::io::Cursor::new(bytes);\n    ...(full match string)...",
            "    let _ = (bytes, output_dir, prefix_candidates);\n    anyhow::bail!(\"zip extraction not available in WASM\")",
        );
```

**File 2:** `runtime/codex-wasm/codex-codemod/src/ast_transforms.rs` (~line 110-113)

Remove this entry from the `STRING_REPLACEMENTS` array:
```rust
    // skills/remote.rs: stub zip extraction function (zip crate stripped)
    ("core/src/skills/remote.rs",
     "    let cursor = std::io::Cursor::new(bytes);\n    ...",
     "    let _ = (bytes, output_dir, prefix_candidates);\n    anyhow::bail!(\"zip extraction not available in WASM\")"),
```

### Step 4: Update the `archive.rs` stub (partial restore)

**File:** `runtime/codex-wasm/codex-codemod/src/ast_transforms.rs` (~line 2390)

The full-file replacement for `package-manager/src/archive.rs` currently stubs
everything. With zip and flate2 available, the zip extraction path can be restored.
However, `tar` is still stripped, so tar.gz extraction remains stubbed.

The original `archive.rs` (270 lines) uses `flate2::read::GzDecoder`, `tar::Archive`,
`zip::ZipArchive`, plus `std::os::unix::fs::PermissionsExt` (unix-only).

**Two options:**

- **Partial restore:** Rewrite the stub to delegate to the real zip extraction code
  while keeping tar.gz stubbed. This is fiddly because the original code interleaves
  zip and tar logic.

- **Defer:** Keep the full stub for now. The `archive.rs` module serves
  `package-manager`, which handles package installation (a different code path from
  `remote.rs` skill downloads). The primary use case (remote skill download via
  `extract_zip_to_dir` in `remote.rs`) is fixed by Steps 1-3.

**Recommendation: Defer.** Focus on `remote.rs` first. The package-manager archive
path can be restored in a follow-up if needed.

### Step 5: Re-run the codemod and rebuild

```sh
cd runtime
cargo run -p codex-codemod -- codex-upstream
cd codex-upstream && git add -A && git commit -m "re-apply codemod: restore zip extraction"
cd ..
moon run runtime:build-wasm
```

The `wasm32-wasip2` build has no C compiler, so if zip-rs accidentally pulls in C
code (zstd, bzip2), the build will fail with a linker error. This is a natural
safety net.

### Step 6: No WIT changes needed

The `extract_zip_to_dir` function uses only:
- `std::io::Cursor` -- available in wasip2
- `zip::ZipArchive` -- pure Rust with correct features
- `std::fs::create_dir_all`, `std::fs::File::create` -- available via wasi:filesystem
- `std::io::copy` -- available in wasip2

No new WIT interfaces are required.

## Summary of Changes

| File | Change |
|------|--------|
| `codex-codemod/src/cargo_toml.rs` | Remove `"zip"` and `"flate2"` from `STRIP_DEPENDENCIES`; add `REWRITE_DEPENDENCY_FEATURES` for zip with safe features |
| `codex-codemod/src/syn_transforms.rs` | Remove the `extract_zip_to_dir` stub in `collect_string_replacement_edits()` |
| `codex-codemod/src/ast_transforms.rs` | Remove the `extract_zip_to_dir` entry from `STRING_REPLACEMENTS` |
| `codex-upstream/` (submodule) | Re-run codemod and commit result |

Files that do NOT change:
- `codex-wasm-tui/Cargo.toml` -- no direct zip dep needed (codemod handles it)
- `codex-wasm-agent/Cargo.toml` -- same
- `ast_transforms.rs` archive.rs stub -- deferred (package-manager path not critical)
- No new shim crate (unlike wasi-sqlx, zip does not need one)

## Testing

1. **Build verification:** `moon run runtime:build-wasm` -- will fail if any C dep
   leaks through.
2. **Runtime verification:** After build, test remote skill download in the TUI
   with a real or mocked zip file.
3. **CI:** Existing CI (`runtime:build-wasm`, `runtime:check`) catches compilation
   issues. No new CI steps needed.

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| `flate2` pulls in C code via feature unification | Low | flate2 defaults to pure-Rust miniz_oxide; verify no other dep enables `zlib` feature |
| Upstream adds `zip` with default features in a new crate | Medium | `REWRITE_DEPENDENCY_FEATURES` in codemod catches this automatically |
| bzip2-compressed zip files fail to extract | Low | bzip2 in zip archives is rare; the error from zip-rs will be clear |
| Cargo feature unification re-enables zstd | Low | zstd crate is still in STRIP_DEPENDENCIES so it cannot be pulled in |

## Estimated Effort

1-2 hours for Steps 1-5 including testing. The `REWRITE_DEPENDENCY_FEATURES`
mechanism (Step 2, Option A) is the only non-trivial code -- it needs ~30 lines
of `toml_edit` manipulation in `cargo_toml.rs`, following the existing pattern of
`SHIM_REDIRECTS` processing.
