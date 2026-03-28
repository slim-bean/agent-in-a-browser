# Plan: Stripe CLI WASM Codemod System

## Context

The stripe-cli fork (`tjfontaine/stripe-cli`, branch `wasip1-wasm-support`) carries a single commit on top of upstream v1.37.3 with ~53 modified files (779 added, 68 removed) to make the Go CLI compile to `GOOS=wasip1 GOARCH=wasm`. Currently these patches live only as git history in the fork, making upstream rebases manual and error-prone.

**Goal**: Create a robust, repeatable Go codemod tool that can be applied to any upstream checkout, so rebasing the fork is: `cd stripe-cli && git rebase upstream/master` then `go run ../codemod`.

## Approach

Keep the submodule pointing at the fork. Add a **Go codemod tool** at `stripe-cli-wasm/codemod/` (standalone `go.mod`) that uses Go's native AST tooling (`go/ast`, `go/parser`, `go/printer`, `golang.org/x/tools/go/ast/astutil`) to apply all modifications correctly and idiomatically.

### Why Go AST over text manipulation

- **`go/parser` + `go/printer`** — parse, transform, and emit canonical Go; output is always `gofmt`-clean
- **`astutil.DeleteImport` / `AddImport`** — handles import grouping, dedup, renaming; verifies imports are truly unused
- **`ast.File.Decls`** — finding and removing a function (with doc comment, receiver) is a simple slice filter
- **Build constraint handling** — `go/ast` models `//go:build` lines as `ast.File.Comments`; insertion/amendment is correct by construction
- **Same language as the target** — anyone working on the fork already has Go; testable with `go test`

## Implementation Steps

### Step 1: Initialize submodule and extract overlays

- `git submodule update --init stripe-cli-wasm/stripe-cli`
- Diff the fork commit against `v1.37.3` to get exact changes
- Categorize: overlay files (new), build tag injections, function extractions, import removals, go.mod patches
- Copy new files into `stripe-cli-wasm/codemod/overlays/` preserving paths

### Step 2: Create `stripe-cli-wasm/codemod/` Go module

```
stripe-cli-wasm/codemod/
  go.mod                    # module stripe-cli-wasm-codemod; requires golang.org/x/tools
  go.sum
  main.go                   # CLI entry point (flags: --target, --verify, --dry-run)
  manifest.go               # Embedded manifest (Go struct, not JSON) defining all modifications
  transforms/
    overlay.go              # Copy overlay files into target
    buildtag.go             # Inject/amend //go:build constraints using go/ast
    extract.go              # Remove functions from files using ast.File.Decls filtering
    imports.go              # Remove/add imports via astutil.DeleteImport/AddImport
    replace.go              # Inline code replacements (AST node matching or targeted text)
    gomod.go                # Patch go.mod (add replace directives, read current versions)
  overlays/                 # New Go files to copy verbatim into stripe-cli/
    pkg/wasmbridge/transport.go
    pkg/wasmbridge/websocket.go
    pkg/wasmbridge/stubs.go
    pkg/wasmbridge/realloc.go
    pkg/stripe/http_client.go
    pkg/stripe/http_client_wasip1.go
    pkg/config/edit_config.go
    pkg/config/edit_config_wasip1.go
    cmd/stripe/telemetry_client.go
    cmd/stripe/telemetry_client_wasip1.go
    pkg/rpcservice/stubs_wasip1.go
    pkg/cmd/daemon_wasip1.go
    pkg/cmd/resource/terminal_wasip1.go
    pkg/cmd/samples/stubs_wasip1.go
    pkg/fixtures/fixtures_wasip1.go
    pkg/useragent/uname_wasip1.go
  codemod_test.go           # Tests: apply to clean checkout, verify diff matches fork
```

### Step 3: Implement `manifest.go` — embedded transformation spec

A Go struct (not JSON) defining all modifications. Keeps the spec type-safe and co-located with the transforms:

```go
var Manifest = Spec{
    BuildTagExclusions: []FilePattern{
        {Glob: "pkg/rpcservice/*.go", Exclude: []string{"*_test.go", "stubs_wasip1.go"}},
        {File: "pkg/cmd/daemon.go"},
        {File: "pkg/cmd/resource/terminal.go"},
        // ... ~30 more files
    },
    BuildTagAmendments: []TagAmendment{
        {File: "pkg/useragent/uname_unix.go", AddConstraint: "!wasip1"},
    },
    FunctionExtractions: []FuncExtraction{
        {File: "pkg/stripe/client.go", FuncName: "newHTTPClient"},
        {File: "pkg/config/config.go", FuncName: "EditConfig", Receiver: "*Config"},
    },
    InlineReplacements: []InlineReplace{
        {File: "cmd/stripe/main.go",
         From: `httpClient := &http.Client{...Timeout...}`,
         To:   `httpClient := newTelemetryHTTPClient()`},
    },
    GoModPatches: []GoModReplace{
        {Module: "github.com/sirupsen/logrus", Replacement: "../patches/logrus"},
    },
}
```

### Step 4: Implement transform functions

Each transform in `transforms/`:

- **`overlay.go`** — uses `embed.FS` or `os.CopyFile` to copy `overlays/**` into target dir. Creates parent dirs. Overwrites unconditionally (idempotent).

- **`buildtag.go`** — `go/parser.ParseFile` → check `f.Comments` for existing `//go:build` → if absent, insert constraint before package clause → `go/printer.Fprint`. Uses `go/build/constraint` package for proper constraint manipulation (AND-ing `!wasip1` into existing constraints).

- **`extract.go`** — `go/parser.ParseFile` → filter `f.Decls` to remove matching `*ast.FuncDecl` (by name + receiver type) → `go/printer.Fprint`. Handles doc comments attached to the func.

- **`imports.go`** — `astutil.DeleteImport(fset, f, importPath)` for each import to remove. Automatically cleans up empty import groups. Checks remaining file references before removing.

- **`replace.go`** — For the `main.go` inline replacement: parse file, use `ast.Inspect` to find the specific `ast.AssignStmt` with `http.Client{...}` composite literal, replace its RHS with a `newTelemetryHTTPClient()` call expression. Remove now-unused imports via `astutil.DeleteImport`.

- **`gomod.go`** — Read `go.mod` as text, parse with `golang.org/x/mod/modfile`, check if replace already exists, add if not, write back. Reads actual logrus version from `require` block dynamically.

### Step 5: CLI entry point (`main.go`)

```
Usage: go run ./codemod [flags]
  --target DIR    Path to stripe-cli checkout (default: ../stripe-cli)
  --verify        After applying, run GOOS=wasip1 GOARCH=wasm go build
  --dry-run       Report what would change without modifying files
```

Phases:
1. Validate target exists and has `go.mod`
2. Apply overlays
3. Apply build tag modifications
4. Apply function extractions + import cleanup
5. Apply inline replacements
6. Apply go.mod patches
7. If `--verify`: run `GOOS=wasip1 GOARCH=wasm go build ./cmd/stripe`

### Step 6: Update `stripe-cli-wasm/moon.yml`

```yaml
apply-codemod:
  script: "cd codemod && go run ."
  inputs:
    - "codemod/**/*"
    - "patches/**/*"
  outputs: []

build-go-wasm:
  deps:
    - "~:apply-codemod"  # Codemod runs before Go build
```

### Step 7: Update README.md

Document:
- Codemod architecture and how to run it
- Rebase workflow: fetch upstream, rebase, run codemod, verify
- How to add new exclusions/stubs: edit `manifest.go`, add overlay files, run `go test`

## File Changes

| File | Action |
|------|--------|
| `stripe-cli-wasm/codemod/go.mod` | Create — standalone Go module |
| `stripe-cli-wasm/codemod/main.go` | Create — CLI entry point |
| `stripe-cli-wasm/codemod/manifest.go` | Create — embedded transformation spec |
| `stripe-cli-wasm/codemod/transforms/*.go` | Create — 6 transform files |
| `stripe-cli-wasm/codemod/overlays/**/*.go` | Create — ~16 overlay files from fork |
| `stripe-cli-wasm/codemod/codemod_test.go` | Create — integration test |
| `stripe-cli-wasm/moon.yml` | Edit — add apply-codemod task |
| `stripe-cli-wasm/README.md` | Edit — document codemod workflow |

## Verification

1. Initialize submodule, run `go run ./codemod --target ../stripe-cli` on clean upstream checkout
2. `GOOS=wasip1 GOARCH=wasm go build ./cmd/stripe` succeeds
3. Diff codemod result against existing fork commit — should be identical
4. `moon run stripe-cli-wasm:build-go-wasm` succeeds
5. `go test ./codemod/...` passes
