# stripe-cli-wasm

Go-compiled Stripe CLI adapted for browser execution as a WASM component.

## Architecture

```
stripe-cli (Go fork)
    ↓ codemod/apply (Go AST transforms)
stripe-cli (patched for wasip1)
    ↓ GOOS=wasip1 GOARCH=wasm go build
stripe.wasm (core module)
    ↓ wasm-tools component new --adapt p1→p2
stripe-component.wasm (wasip2 component)
    ↓ JCO transpile (scripts/transpile.mjs)
packages/wasm-stripe/wasm/stripe-module.js
    ↓ lazy-loaded by frontend
Browser terminal: `stripe customers list`
```

## Codemod

The `codemod/` directory contains a Go tool that transforms a clean upstream
stripe-cli checkout into a wasip1-compatible build. It uses Go's native AST
tooling (`go/ast`, `go/parser`, `golang.org/x/tools/go/ast/astutil`) for
correct, idempotent transformations.

### What the codemod does

1. **Copies overlay files** — new Go files (wasmbridge package, wasip1 stubs,
   platform-split function files) into the stripe-cli source tree
2. **Injects build tags** — prepends `//go:build !wasip1` to files that use
   unavailable syscalls (gRPC, subprocess, git, terminal hardware)
3. **Extracts functions** — moves `newHTTPClient` and `EditConfig` out of their
   original files into platform-split pairs (`_wasip1.go` / `!wasip1`)
4. **Removes unused imports** — cleans up imports left behind by extraction
5. **Patches go.mod** — adds `replace` directive for WASI-compatible logrus

### Running the codemod

```sh
cd codemod && go run . --target ../stripe-cli
# Or with build verification:
cd codemod && go run . --target ../stripe-cli --verify
# Dry run (no changes):
cd codemod && go run . --target ../stripe-cli --dry-run
```

### Updating to a new upstream version

```sh
cd stripe-cli
git fetch upstream
git rebase upstream/master
# Re-apply codemod (idempotent — skips already-applied changes)
cd ../codemod && go run . --target ../stripe-cli --verify
# Commit and push
cd ../stripe-cli && git add -A && git commit -m "feat: update wasip1 WASM support"
git push origin wasip1-wasm-support --force-with-lease
```

If upstream introduced new files that break the WASM build, update
`codemod/manifest.go` to add new exclusions or stubs, add any needed overlay
files to `codemod/overlays/`, then re-run.

## Setup

### 1. Initialize the submodule

```sh
git submodule update --init stripe-cli-wasm/stripe-cli
```

### 2. Download the p1→p2 adapter

```sh
curl -L -o adapters/wasi_snapshot_preview1.command.wasm \
  https://github.com/bytecodealliance/wasmtime/releases/latest/download/wasi_snapshot_preview1.command.wasm
```

### 3. Build via Moon

```sh
# Full pipeline: codemod → Go build → adapt → transpile
moon run stripe-cli-wasm:build-go-wasm stripe-cli-wasm:adapt-component \
  stripe-cli-wasm:copy-to-target wasm-stripe:transpile wasm-stripe:transpile-sync
```

Or step by step:
```sh
moon run stripe-cli-wasm:apply-codemod       # Step 0: Apply WASM codemod
moon run stripe-cli-wasm:build-go-wasm       # Step 1: Go → wasip1 WASM
moon run stripe-cli-wasm:adapt-component     # Step 2: wasip1 → wasip2 component
moon run stripe-cli-wasm:copy-to-target      # Step 3: Copy to shared target
moon run wasm-stripe:transpile               # Step 4: JCO transpile (JSPI)
moon run wasm-stripe:transpile-sync          # Step 5: JCO transpile (sync)
```

## Directory Structure

```
stripe-cli-wasm/
├── moon.yml                    # Moon build tasks
├── README.md
├── stripe-cli/                 # Fork of stripe/stripe-cli (git submodule)
├── codemod/
│   ├── main.go                 # CLI entry point
│   ├── manifest.go             # Declarative spec of all modifications
│   ├── overlay.go              # Copy overlay files
│   ├── buildtag.go             # Build tag injection/amendment
│   ├── extract.go              # Function extraction via go/ast
│   ├── replace.go              # Inline replacements + import cleanup
│   ├── gomod.go                # go.mod patching via x/mod/modfile
│   └── overlays/               # New Go files copied into stripe-cli
│       ├── pkg/wasmbridge/     # HTTP/WS bridge via //go:wasmimport
│       ├── pkg/stripe/         # Platform-split HTTP client
│       ├── pkg/config/         # Platform-split EditConfig
│       ├── cmd/stripe/         # Platform-split telemetry client
│       └── ...                 # wasip1 stub files
├── patches/
│   └── logrus/                 # WASI-compatible logrus fork
├── wasm-bridge/                # Reference copies of bridge Go files
├── wit/
│   ├── http-bridge.wit         # WIT for http_bridge imports
│   └── ws-bridge.wit           # WIT for ws_bridge imports
└── adapters/
    └── wasi_snapshot_preview1.command.wasm  # p1→p2 adapter (downloaded)
```

## JS Shims

The host-side implementations of the WASM imports live in `packages/wasi-shims/src/`:

- `http-bridge-impl.ts` — Implements `http_bridge` using browser `fetch()`
- `ws-bridge-impl.ts` — Implements `ws_bridge` using browser `WebSocket`

These are mapped by JCO in `scripts/transpile.mjs` via `--map` flags.

## Risks & Known Issues

- **Binary size**: Go WASM binaries are large (~40MB). Consider `wasm-opt -Oz` and brotli compression.
- **Goroutine scheduler**: Go's goroutine scheduler in WASM is single-threaded. Concurrent HTTP requests are serialized.
- **CORS**: Browser cross-origin restrictions apply. Stripe API calls may need the CORS proxy (`/cors-proxy`).
- **Custom sections**: Go's wasip1 output may include custom sections that `wasm-tools component new` doesn't handle. Use `wasm-tools strip` if needed.
