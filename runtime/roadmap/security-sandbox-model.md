# Security/Sandbox Model: Codex TUI WASM Build

Updated: 2026-03-28

## 1. Current Architecture: How Things Actually Work

### 1.1 Execution Data Flow

The Codex TUI runs as a `wasm32-wasip2` component inside a browser Worker. All
external interactions are mediated by JS shims that implement WIT interfaces:

```
Codex TUI WASM (wasm32-wasip2)
  |
  +-- shell-exec WIT import
  |     -> shell-exec-impl.ts (ExecHandler callback)
  |       -> tui-loader.ts / wasm-worker.ts registers handler
  |         -> fetchFromSandbox('/mcp/message', { tools/call: run_command })
  |           -> SharedSandboxWorker.ts handles 'fetch' message
  |             -> callWasmMcpServerFetch(request)
  |               -> ts-runtime-mcp WASM (MCP server)
  |                 -> run_command tool -> interactive.rs
  |                   -> shell builtin dispatch (ls, cat, grep, etc.)
  |
  +-- wasi:http/outgoing-handler WIT import
  |     -> wasi-http-impl.ts (TransportHandler callback)
  |       -> tui-loader.ts sets handler -> fetchFromSandbox()
  |         -> SharedSandboxWorker routes to callWasmMcpServerFetch()
  |       OR: direct fetch() for non-MCP URLs (LLM API calls)
  |
  +-- codex:tui/websocket WIT import
  |     -> websocket-impl.ts
  |       -> browser native WebSocket API (for OpenAI Responses WS transport)
  |
  +-- wasi:filesystem WIT import
  |     -> opfs-filesystem-impl.ts
  |       -> OPFS (Origin Private File System)
  |
  +-- wasi:cli/* WIT imports
        -> ghostty-cli-shim.ts (terminal I/O via ghostty-web)
```

### 1.2 WIT Capabilities Granted (`codex-wasm-tui/wit/world.wit`)

The codex-tui world imports these interfaces:

| WIT Interface | JS Shim | What It Does |
|---|---|---|
| `codex:tui/shell-exec` | `shell-exec-impl.ts` | Execute commands via MCP `run_command` tool |
| `codex:tui/websocket` | `websocket-impl.ts` | Open/send/recv/close WebSocket connections |
| `wasi:http/outgoing-handler` | `wasi-http-impl.ts` | HTTP requests (LLM API, MCP fetch) |
| `wasi:filesystem/*` | `opfs-filesystem-impl.ts` | File I/O backed by OPFS |
| `wasi:cli/*` | `ghostty-cli-shim.ts` | Terminal stdin/stdout/stderr, environment |
| `wasi:clocks/*` | JCO defaults | Monotonic + wall clock |
| `wasi:random/*` | JCO defaults | Random bytes for session IDs |
| `terminal:info/size` | ghostty-cli-shim | Terminal dimensions |

### 1.3 SharedWorker Role

`SharedSandboxWorker.ts` is the central hub. It:
- Acquires the OPFS root handle and initializes the filesystem shim
- Loads the MCP server WASM module (ts-runtime-mcp)
- Handles `fetch` and `fetch-simple` messages by routing to `callWasmMcpServerFetch()`
- Manages connected ports for multiple tabs
- Does NOT perform any security policy checks

### 1.4 Shell Execution Path (Detailed)

1. Codex TUI Rust calls `exec(program, args, env, stdin, timeout_ms)` via WIT import
2. JCO routes to `shell-exec-impl.ts::exec()` (JSPI suspends WASM stack)
3. `exec()` calls the registered `ExecHandler` (set by `tui-loader.ts`)
4. The handler builds a JSON-RPC `tools/call` request for `run_command`
5. Request goes to `fetchFromSandbox()` -> SharedWorker -> `callWasmMcpServerFetch()`
6. The MCP server WASM dispatches to the `run_command` tool
7. `run_command` calls `interactive.rs::run_command_string()` which dispatches to
   built-in shell commands (ls, cat, grep, cd, etc.) implemented in Rust
8. Result (exit code, stdout, stderr) flows back through the entire chain

**No approval check exists anywhere in this chain.**

### 1.5 HTTP Request Path (Detailed)

1. Codex TUI Rust makes HTTP request via `wasi:http/outgoing-handler`
2. JCO routes to `wasi-http-impl.ts` `handle()` function
3. If a `transportHandler` is set (it is, by `tui-loader.ts`):
   - MCP-bound requests go through `fetchFromSandbox()` -> SharedWorker
   - SharedWorker routes to `callWasmMcpServerFetch()`
4. If no transport handler matches, falls through to `fetch()` (LLM API calls)

**No domain filtering or approval exists. All requests pass through.**

### 1.6 WebSocket Path

1. WASM calls `connect(url, protocols)` via WIT
2. `websocket-impl.ts` creates a native browser `WebSocket`
3. No URL validation, no domain check

---

## 2. Upstream Codex Security Model (Reference)

Upstream Codex has three enforcement layers:

### 2.1 Exec Policy (`execpolicy` crate)

The `codex-execpolicy` crate (at `codex-rs/execpolicy/`) is a Starlark-based
rule engine. It:
- Parses `.rules` files (Starlark DSL) that map command prefixes to decisions
- `Policy::check(cmd, heuristics_fallback)` returns `Evaluation { decision, matched_rules }`
- Decisions: `Allow`, `Prompt`, `Forbidden`
- Heuristics fallback classifies unknown commands via `is_safe_command` / `command_might_be_dangerous`
- Network rules with per-host/protocol decisions
- **This crate compiles for wasm32-wasip2** (pure Rust + starlark, no OS deps)

### 2.2 OS-Level Sandbox

Three platform backends (seatbelt/macOS, landlock/Linux, Windows Sandbox) wrap
process execution. **All three are stubbed to no-ops** in the WASM build via
`codex-codemod` `REPLACE_FILES` entries.

### 2.3 Network Proxy

The `network-proxy` crate runs an HTTP/SOCKS5 proxy for domain filtering.
**Stubbed to no-ops** in the WASM build (build() returns error, all methods no-op).

### 2.4 Approval Flow (Upstream)

In `core/src/tools/sandboxing.rs`:
1. Tool runtime prepares a `CommandSpec`
2. `SandboxManager` transforms it into sandboxed `ExecRequest`
3. `ExecPolicyManager::evaluate()` checks rules -> `Allow`/`Prompt`/`Forbidden`
4. If `Prompt`: TUI shows `ApprovalRequest::Exec` overlay
   (`tui/src/bottom_pane/approval_overlay.rs`)
5. User presses y/n, `ReviewDecision` cached in `ApprovalStore`
6. Guardian review (optional AI safety layer)

**This entire flow exists and compiles in the WASM build**, but:
- The OS sandbox parts are stubbed (no-op)
- The exec policy rules engine works (pure Rust)
- The TUI approval overlay UI works (ratatui)
- The actual command execution bypasses all of this because shell-exec goes
  through the WIT interface to the JS shim, not through the upstream Rust exec path

---

## 3. Browser Security Guarantees (Free)

| Security Property | Browser Mechanism | Notes |
|---|---|---|
| Memory isolation | WASM linear memory sandbox | Cannot access JS heap |
| No raw sockets | Browser network stack only | No TCP/UDP direct access |
| Origin-scoped storage | OPFS origin isolation | Per-origin virtual FS |
| No arbitrary JS exec | WASM can only call WIT imports | Host controls all APIs |
| Cross-origin restrictions | CORS on fetch() | Partial network restriction |
| Worker thread isolation | Separate global scope | No DOM, cookies, localStorage |
| No native process spawn | Browser has no child_process | All "commands" are simulated |

---

## 4. Security Gaps Analysis

### 4.1 CRITICAL: No Command Approval Flow

**Current state**: `shell-exec-impl.ts::exec()` calls the registered `ExecHandler`
unconditionally. The handler immediately builds a JSON-RPC request to the MCP
`run_command` tool. No policy check, no user prompt.

**Why this matters**: The LLM agent calls shell commands to accomplish tasks. Without
gating, it can execute anything the MCP shell supports -- file modifications,
reading sensitive files, running arbitrary scripts. While the blast radius is
limited to OPFS (not the real filesystem), users expect to review commands before
they run.

**Existing assets**:
- The exec policy engine compiles and works in WASM (pure Rust)
- The TUI approval overlay UI exists (`approval_overlay.rs`)
- JSPI can suspend/resume the WASM stack (already used for async shims)
- The `ExecHandler` is async and returns a Promise

### 4.2 HIGH: No Network Domain Restrictions

**Current state**: `wasi-http-impl.ts` passes all requests through via
`transportHandler` or `fetch()`. `websocket-impl.ts` opens connections to any URL.

**Why this matters**: The agent makes LLM API calls (needed), but could also
exfiltrate data to any CORS-permissive endpoint. CORS is not sufficient because
data exfiltration only requires the request to be sent.

**Existing assets**:
- The exec policy crate has `NetworkRule` with host/protocol/decision
- `Policy::compiled_network_domains()` returns (allowed, denied) domain lists
- The transport handler pattern in `wasi-http-impl.ts` is the right interception point

### 4.3 MEDIUM: No File Access Policy

**Current state**: OPFS filesystem shim exposes the entire origin-scoped virtual
filesystem. Single root `/` preopen with full read/write.

**Mitigation**: OPFS is already origin-scoped. The virtual filesystem cannot access
the user's real files. This is significantly less risky than desktop Codex.

### 4.4 MEDIUM: Volatile Credential Storage

**Current state**: Keyring is an in-memory `HashMap` (stubbed via codex-codemod).
API keys lost on page reload.

### 4.5 LOW: No Resource Limits

**Current state**: No command timeout enforcement beyond browser tab limits.

---

## 5. Implementation Plan

### Key Design Principle

The JS shim layer is the correct enforcement point. WASM cannot call browser APIs
except through imports the host provides. In-WASM policy checks are defense-in-depth
but the JS shim is the trust boundary.

### Phase 1: Command Approval (Most Important Gap)

**Goal**: Add approval gating to the shell-exec shim so the user sees and approves
commands before they run.

**Approach**: The upstream exec policy engine (Starlark rules) is too heavyweight
to port to TypeScript and depends on the full Rust config system. Instead, implement
a lightweight TypeScript policy in the shim layer with the same `Allow`/`Prompt`/`Deny`
semantics, using simple prefix matching.

#### Step 1a: Command Policy Engine

**File**: `packages/wasi-shims/src/command-policy.ts` (new)

```typescript
export type Decision = 'allow' | 'prompt' | 'deny';

export interface CommandRule {
    prefix: string[];  // e.g., ['ls'], ['git', 'status']
    decision: Decision;
}

// Default safe commands (read-only operations)
const DEFAULT_ALLOW: string[][] = [
    ['ls'], ['cat'], ['head'], ['tail'], ['wc'],
    ['find'], ['grep'], ['rg'], ['fd'],
    ['pwd'], ['echo'], ['env'], ['which'],
    ['git', 'status'], ['git', 'log'], ['git', 'diff'],
    ['git', 'show'], ['git', 'branch'],
    ['tree'], ['file'], ['stat'], ['du'],
];

// Always denied
const DEFAULT_DENY: string[][] = [
    ['curl'], ['wget'],  // network access should use HTTP shim
];

export class CommandPolicy {
    private rules: CommandRule[] = [];
    private sessionApprovals = new Map<string, Decision>();

    constructor() {
        for (const prefix of DEFAULT_ALLOW) {
            this.rules.push({ prefix, decision: 'allow' });
        }
        for (const prefix of DEFAULT_DENY) {
            this.rules.push({ prefix, decision: 'deny' });
        }
    }

    evaluate(program: string, args: string[]): Decision {
        const cmd = [program, ...args];
        const key = cmd.join(' ');

        // Check session cache first
        const cached = this.sessionApprovals.get(key);
        if (cached) return cached;

        // Check rules (longest prefix match wins)
        let bestMatch: CommandRule | null = null;
        let bestLen = 0;
        for (const rule of this.rules) {
            if (rule.prefix.length > bestLen && this.prefixMatches(rule.prefix, cmd)) {
                bestMatch = rule;
                bestLen = rule.prefix.length;
            }
        }

        return bestMatch?.decision ?? 'prompt';  // default: prompt for unknown
    }

    approveForSession(program: string, args: string[]): void {
        this.sessionApprovals.set([program, ...args].join(' '), 'allow');
    }

    private prefixMatches(prefix: string[], cmd: string[]): boolean {
        if (cmd.length < prefix.length) return false;
        return prefix.every((tok, i) => tok === cmd[i]);
    }
}
```

#### Step 1b: Wire Policy Into Shell Exec Shim

**File**: `packages/wasi-shims/src/shell-exec-impl.ts` (modify)

Add policy check before calling `execHandler`. When the decision is `prompt`,
call an `approvalHandler` callback that the host registers (similar to `setExecHandler`).

```typescript
// New exports added to shell-exec-impl.ts:
export type ApprovalHandler = (
    program: string,
    args: string[],
    env: ExecEnv,
) => Promise<'allow' | 'deny' | 'allow-session'>;

let approvalHandler: ApprovalHandler | null = null;

export function setApprovalHandler(handler: ApprovalHandler): void {
    approvalHandler = handler;
}

// Modified exec():
export async function exec(program, args, env, stdin, timeoutMs) {
    if (!execHandler) throw new Error('No shell exec handler registered.');

    const decision = commandPolicy.evaluate(program, args);

    if (decision === 'deny') {
        return { exitCode: 1, stdout: new Uint8Array(0),
                 stderr: new TextEncoder().encode(`Command denied by policy: ${program}`) };
    }

    if (decision === 'prompt' && approvalHandler) {
        const approval = await approvalHandler(program, args, env);
        if (approval === 'deny') {
            return { exitCode: 1, stdout: new Uint8Array(0),
                     stderr: new TextEncoder().encode('Command denied by user') };
        }
        if (approval === 'allow-session') {
            commandPolicy.approveForSession(program, args);
        }
    }

    return execHandler(program, args, env, stdin, timeoutMs);
}
```

#### Step 1c: Approval UI Bridge

**Option A (Simplest)**: Use the upstream TUI approval overlay.

The Codex TUI already has `approval_overlay.rs` with full keyboard-driven
approval UI. The upstream approval flow works like this:

1. `codex-core` emits an `ApprovalRequest` event
2. TUI renders the approval overlay (shows command, y/n/always keys)
3. User responds, TUI sends `ReviewDecision` back

The challenge: in the WASM build, shell commands bypass `codex-core`'s exec path.
Commands go: WASM -> WIT -> JS shim -> MCP tool. The upstream approval flow
triggers inside `codex-core`'s tool orchestrator, which never sees these commands.

**Option B (Recommended)**: Approval in the JS shim, with UI forwarded to the
main thread.

The `ApprovalHandler` callback (registered by `tui-loader.ts`) sends a message to
the main thread via `postMessage`. The main thread renders a simple confirmation
dialog or injects the approval request into the TUI's terminal stream.

**File**: `frontend/src/wasm/tui/tui-loader.ts` (modify)

```typescript
import { setApprovalHandler } from '@tjfontaine/wasi-shims/shell-exec-impl.js';

// Register approval handler that prompts via the TUI
setApprovalHandler(async (program, args, env) => {
    // Option 1: Send to main thread for a browser dialog
    // Option 2: Write a prompt to the terminal and read response
    // Option 3: Use a dedicated MessageChannel to the UI
    return new Promise((resolve) => {
        const channel = new MessageChannel();
        // Post to main thread with the command details
        self.postMessage({
            type: 'approval-request',
            command: [program, ...args].join(' '),
            cwd: env.cwd,
            port: channel.port2,
        }, [channel.port2]);

        channel.port1.onmessage = (e) => {
            resolve(e.data.decision); // 'allow' | 'deny' | 'allow-session'
        };
    });
});
```

#### Step 1d: Main Thread Approval UI

**File**: `frontend/src/agent/sandbox.ts` (modify) or new component

Listen for `approval-request` messages from the worker and render a confirmation.
This can be a simple browser `confirm()` dialog initially, upgraded to a styled
modal later.

#### Files Changed (Phase 1)

| File | Change |
|------|--------|
| `packages/wasi-shims/src/command-policy.ts` | New: lightweight command policy engine |
| `packages/wasi-shims/src/shell-exec-impl.ts` | Add: policy check + approval handler before exec |
| `frontend/src/wasm/tui/tui-loader.ts` | Add: register approval handler, message to main thread |
| `frontend/src/workers/wasm-worker.ts` | Add: same approval handler registration |
| `frontend/src/agent/sandbox.ts` | Add: listen for approval-request, render UI |
| `packages/wasi-shims/package.json` | Add: export for `command-policy.ts` |

### Phase 2: Network Domain Policy

**Goal**: Gate HTTP requests and WebSocket connections by domain.

#### Step 2a: Network Policy Engine

**File**: `packages/wasi-shims/src/network-policy.ts` (new)

```typescript
export class NetworkPolicy {
    private allowedDomains = new Set<string>();
    private deniedDomains = new Set<string>();
    private sessionApprovals = new Set<string>();

    constructor(llmApiDomain?: string) {
        // Always allow the configured LLM API endpoint
        if (llmApiDomain) this.allowedDomains.add(llmApiDomain);
        // Always allow localhost (MCP server)
        this.allowedDomains.add('localhost');
    }

    evaluate(url: string): Decision {
        const hostname = new URL(url).hostname;
        if (this.allowedDomains.has(hostname)) return 'allow';
        if (this.sessionApprovals.has(hostname)) return 'allow';
        if (this.deniedDomains.has(hostname)) return 'deny';
        return 'prompt';
    }

    approveForSession(url: string): void {
        this.sessionApprovals.add(new URL(url).hostname);
    }
}
```

#### Step 2b: Wire Into HTTP and WebSocket Shims

**File**: `packages/wasi-shims/src/wasi-http-impl.ts` (modify)

Add a network policy check before the transport handler or `fetch()` call.
The check happens in the `handle()` function that processes outgoing requests.

**File**: `packages/wasi-shims/src/websocket-impl.ts` (modify)

Add a network policy check in `connect()` before creating the `WebSocket`.

#### Files Changed (Phase 2)

| File | Change |
|------|--------|
| `packages/wasi-shims/src/network-policy.ts` | New: domain-based network policy |
| `packages/wasi-shims/src/wasi-http-impl.ts` | Add: domain check before transport/fetch |
| `packages/wasi-shims/src/websocket-impl.ts` | Add: domain check before WebSocket connect |
| `frontend/src/wasm/tui/tui-loader.ts` | Add: configure LLM API domain in network policy |

### Phase 3: File Access Policy + Credentials

#### Step 3a: OPFS Path Policy

**File**: `packages/wasi-shims/src/opfs-filesystem-impl.ts` (modify)

Wrap `Descriptor` methods (openAt, readViaStream, writeViaStream,
createDirectoryAt, removeDirectoryAt, unlinkFileAt) with path normalization
and zone checks:

| Path Zone | Access | Purpose |
|-----------|--------|---------|
| `/workspace/` | Read-Write | User project directory |
| `/tmp/` | Read-Write | Ephemeral scratch |
| `/home/.codex/` | Read-Write | Codex state and history |
| `/home/.config/` | Read-Only | Configuration |
| All others | Read-Only | Default deny writes |

#### Step 3b: Persistent Encrypted Credential Storage

**File**: `packages/wasi-shims/src/encrypted-keyring.ts` (new)

Replace the in-memory `HashMap` keyring stub with IndexedDB storage encrypted
via the Web Crypto API. Key derivation from user-provided passphrase or
auto-generated key stored in a secure context.

#### Files Changed (Phase 3)

| File | Change |
|------|--------|
| `packages/wasi-shims/src/opfs-filesystem-impl.ts` | Add: path zone enforcement |
| `packages/wasi-shims/src/opfs-filesystem-sync-impl.ts` | Same for Safari path |
| `packages/wasi-shims/src/encrypted-keyring.ts` | New: IndexedDB + WebCrypto keyring |

### Phase 4: Polish

- **Execution timeouts**: AbortController in shell-exec-impl.ts, Worker
  termination after configurable timeout
- **Output size caps**: Truncate stdout/stderr in the exec handler
- **Audit logging**: Record all policy decisions to a structured log, surface
  in a "Security" panel
- **Policy configuration UI**: Settings for managing allowlists and rules
- **CSP header**: Deploy `connect-src` directive in `frontend/vite.config.ts`
  or server config for defense-in-depth

---

## 6. Key Questions Answered

### Q: Does the exec policy crate compile and work in WASM?

**Yes, partially.** The `codex-execpolicy` crate itself is pure Rust with no OS
deps and compiles for `wasm32-wasip2`. However:

- It depends on `starlark` (for parsing `.rules` files), which does compile
- It depends on `codex-utils-absolute-path` for path handling
- The `ExecPolicyManager` in `codex-core` depends on `tokio::fs` for reading
  rule files from disk, which is stubbed in WASM
- The rule parser works, but rule files would need to come from OPFS or be
  embedded

**Verdict**: The rule engine works but loading rules from the filesystem requires
adaptation. For the browser, a TypeScript-native policy is simpler and more
maintainable as a first step. The Rust engine could be used as a defense-in-depth
layer later.

### Q: Can we wire the existing approval UI into the shell-exec shim?

**Not directly.** The upstream approval flow is tightly coupled to `codex-core`'s
tool orchestrator:

1. `codex-core` calls `ExecPolicyManager::evaluate()` before executing commands
2. If `Prompt`, it sends `Op::ExecApprovalRequest` through the `codex_delegate`
3. The TUI receives this event and shows `ApprovalRequest::Exec` overlay
4. User response flows back through `codex_delegate::review_exec()`

The problem: in the WASM build, commands go through the WIT `shell-exec`
interface directly. `codex-core`'s tool orchestrator never sees these commands
because they are dispatched by the JS shim to the MCP server. The upstream
approval flow cannot intercept them.

**Solution**: Implement approval in the JS shim layer where we can intercept
before the MCP `run_command` is called. The approval UI can be either:
- A browser-side dialog (simplest)
- A message injected into the terminal stream (better UX, more work)
- A dedicated overlay component (best UX, most work)

### Q: What is the single most important security gap to close?

**Command approval.** The agent can execute arbitrary shell commands without
user awareness. While the blast radius is limited to OPFS, this violates user
expectations from desktop Codex where every non-trivial command requires
explicit approval.

The fix is contained to 3-4 files and can be done incrementally:
1. Add `CommandPolicy` class to the wasi-shims package
2. Wire it into `shell-exec-impl.ts` (5 lines of logic)
3. Register an `ApprovalHandler` in `tui-loader.ts`
4. Handle the approval request in the main thread

---

## 7. Architecture Diagram (Target State)

```
User (Browser Tab)
  |
  | postMessage (approval requests/responses)
  |
Worker (WASM Host)
  |
  +-- Policy Layer (TypeScript, in shim modules)
  |   +-- CommandPolicy  (shell-exec-impl.ts)
  |   +-- NetworkPolicy  (wasi-http-impl.ts, websocket-impl.ts)
  |   +-- FilePolicy     (opfs-filesystem-impl.ts)
  |   +-- ApprovalCache  (session-scoped, in-memory)
  |
  +-- WASM Runtime (Sandboxed)
  |   +-- Codex TUI (codex-wasm-tui)
  |   +-- Upstream execpolicy (defense-in-depth, not primary enforcement)
  |
  +-- Host Shims (mediate ALL WASM->outside access)
  |   +-- shell-exec-impl.ts   -> CommandPolicy check -> ExecHandler
  |   +-- wasi-http-impl.ts    -> NetworkPolicy check -> fetch()
  |   +-- websocket-impl.ts    -> NetworkPolicy check -> WebSocket()
  |   +-- opfs-filesystem-*.ts -> FilePolicy check -> OPFS
  |
  +-- SharedSandboxWorker (MCP routing, OPFS init)
      +-- ts-runtime-mcp WASM (shell tools, file tools)
```

**Why the JS shim layer is the trust boundary**: WASM can only interact with
the outside world through WIT imports that the host provides. Even if the Rust
exec policy engine were compromised or bypassed, the JS shim would still
enforce its policy. The WASM module has no way to call `fetch()` or access OPFS
except through these shims.

---

## 8. Key Files Reference

| Component | File | Current State |
|-----------|------|--------------|
| Shell exec shim | `packages/wasi-shims/src/shell-exec-impl.ts` | No policy check, passes through to ExecHandler |
| HTTP shim | `packages/wasi-shims/src/wasi-http-impl.ts` | No domain check, passes through to transport/fetch |
| WebSocket shim | `packages/wasi-shims/src/websocket-impl.ts` | No URL check, creates native WebSocket |
| OPFS filesystem shim | `packages/wasi-shims/src/opfs-filesystem-impl.ts` | No path policy, full OPFS access |
| OPFS filesystem (sync) | `packages/wasi-shims/src/opfs-filesystem-sync-impl.ts` | Same, Safari variant |
| SharedWorker | `frontend/src/workers/SharedSandboxWorker.ts` | Routes MCP fetch, no policy enforcement |
| TUI loader | `frontend/src/wasm/tui/tui-loader.ts` | Registers exec handler and transport handler |
| WASM worker | `frontend/src/workers/wasm-worker.ts` | Alternate loader, same exec handler pattern |
| Sandbox client | `frontend/src/agent/sandbox.ts` | Manages SharedWorker connection |
| WIT world | `runtime/codex-wasm/codex-wasm-tui/wit/world.wit` | Declares all WIT imports |
| Upstream exec policy | `runtime/codex-upstream/codex-rs/execpolicy/` | Starlark rule engine (compiles for WASM) |
| Upstream approval UI | `runtime/codex-upstream/codex-rs/tui/src/bottom_pane/approval_overlay.rs` | TUI approval dialog |
| Upstream approval flow | `runtime/codex-upstream/codex-rs/core/src/tools/sandboxing.rs` | ApprovalStore + cached decisions |
| Upstream exec policy mgr | `runtime/codex-upstream/codex-rs/core/src/exec_policy.rs` | Rule loading + evaluation orchestration |
| Codemod stubs | `runtime/codex-wasm/codex-codemod/src/ast_transforms.rs` | Lists all stubbed modules |
| JCO transpile config | `scripts/transpile.mjs` | Controls WIT->JS mapping |
