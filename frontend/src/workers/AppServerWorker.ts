/**
 * AppServerWorker - Dedicated Worker hosting the codex-wasm-app-server WASM module.
 *
 * Replaces main-thread execution of the WASM. Registers all shim handlers,
 * runs OPFS/credential-store natively (createSyncAccessHandle works in Workers),
 * and proxies sandbox communication (transport, exec, MCP) through the main
 * thread via postMessage.
 */

// WASM module imports (transpiled with JCO)
import {
    start,
    protocol,
    pushAuthCallback,
} from '../wasm/codex-app-server/codex-wasm-app-server.js';

// Shim imports
import { setEnvironment } from '@tjfontaine/wasi-shims/ghostty-cli-shim.js';
import { setTransportHandler, setNetworkApprovalHandler } from '@tjfontaine/wasi-shims/wasi-http-impl.js';
import { setExecHandler, setApprovalHandler } from '@tjfontaine/wasi-shims/shell-exec-impl.js';
import {
    setPtyHandler,
    type PtyHandler,
    type PtyStartParams,
    type PtyStartResult,
    type PtyReadResult,
    type PtyWriteResult,
} from '@tjfontaine/wasi-shims/shell-pty-impl.js';
import { setEventHandler } from '@tjfontaine/wasi-shims/event-sink-impl.js';
import { initFilesystem } from '@tjfontaine/wasi-shims/opfs-filesystem-impl.js';

// ==========================================================================
// Call ID Infrastructure
// ==========================================================================

let callIdCounter = 0;
const pendingCalls = new Map<string, { resolve: (value: unknown) => void; reject: (reason: unknown) => void }>();

function nextCallId(prefix: string): string {
    return `${prefix}-${++callIdCounter}`;
}

/** Post a message to the main thread and await a response matched by callId. */
function requestFromMain<T>(type: string, data: Record<string, unknown>, transfer?: Transferable[]): Promise<T> {
    const callId = nextCallId(type);
    return new Promise<T>((resolve, reject) => {
        pendingCalls.set(callId, { resolve: resolve as (v: unknown) => void, reject });
        const msg = { type, callId, ...data };
        if (transfer) {
            self.postMessage(msg, transfer);
        } else {
            self.postMessage(msg);
        }
    });
}

// ==========================================================================
// Transport Handler (proxy through main thread)
// ==========================================================================

setTransportHandler(async (
    method: string,
    url: string,
    headers: Record<string, string>,
    body: Uint8Array | null,
) => {
    const result = await requestFromMain<{
        status: number;
        headers: [string, Uint8Array][];
        body: Uint8Array;
    }>('transport-request', {
        method,
        url,
        headers,
        body: body ? body.buffer : null,
    }, body ? [body.buffer] : undefined);
    return result;
});

// ==========================================================================
// Exec Handler (proxy through main thread)
// ==========================================================================

setExecHandler(async (
    program: string,
    args: string[],
    env: { cwd: string },
    stdin: Uint8Array | undefined,
    timeoutMs: number | undefined,
) => {
    const result = await requestFromMain<{
        exitCode: number;
        stdout: Uint8Array;
        stderr: Uint8Array;
    }>('exec-request', {
        program,
        args,
        cwd: env.cwd,
        stdin: stdin ? stdin.buffer : null,
        timeoutMs: timeoutMs ?? 30000,
    }, stdin ? [stdin.buffer] : undefined);
    return result;
});

// ==========================================================================
// Approval Handlers (proxy through main thread)
// ==========================================================================

setApprovalHandler(async (program: string, args: string[], env: { cwd: string }) => {
    const result = await requestFromMain<{ decision: string }>('approval-request', {
        program,
        args,
        cwd: env.cwd,
    });
    return result.decision as 'allow' | 'allow-session' | 'deny';
});

setNetworkApprovalHandler(async (url: string, method: string) => {
    const result = await requestFromMain<{ decision: string }>('network-approval-request', {
        url,
        method,
    });
    return result.decision as 'allow' | 'allow-session' | 'deny';
});

// ==========================================================================
// Event Handler
// ==========================================================================

setEventHandler((json: string) => {
    self.postMessage({ type: 'event', json });
});

// ==========================================================================
// MCP Tool Call Helper (used by PtySessionManager)
// ==========================================================================

async function mcpToolCall(name: string, args: Record<string, unknown>): Promise<unknown> {
    const body = JSON.stringify({
        jsonrpc: '2.0',
        id: Date.now(),
        method: 'tools/call',
        params: { name, arguments: args },
    });
    const result = await requestFromMain<{ status: number; body: string }>('mcp-request', {
        path: '/mcp/message',
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body,
    });
    const json = JSON.parse(result.body) as { error?: { message?: string }; result?: unknown };
    if (json.error) {
        throw new Error(json.error.message ?? 'MCP tool call failed');
    }
    return json.result;
}

// ==========================================================================
// PTY Session Manager
// ==========================================================================

class PtySessionManager implements PtyHandler {
    private sessions = new Map<string, {
        /** Output chunks buffered for read() */
        chunks: { seq: bigint; data: Uint8Array }[];
        /** Next sequence number for output chunks */
        nextSeq: bigint;
        /** Whether the session has been terminated */
        exited: boolean;
        /** Exit code if exited */
        exitCode: number | undefined;
        /** Wake counter -- increments when new output is available */
        wakeSeq: bigint;
        /** Resolvers waiting for new output (pollWake callers) */
        wakeResolvers: (() => void)[];
    }>();

    async start(params: PtyStartParams): Promise<PtyStartResult> {
        console.log('[PTY] start:', params.processId, 'argv:', params.argv, 'cwd:', params.cwd);

        await mcpToolCall('pty_start', {
            process_id: params.processId,
            cwd: params.cwd || '/workspace',
            env: params.env.length > 0 ? JSON.stringify(params.env) : undefined,
        });

        this.sessions.set(params.processId, {
            chunks: [],
            nextSeq: 0n,
            exited: false,
            exitCode: undefined,
            wakeSeq: 0n,
            wakeResolvers: [],
        });

        if (params.argv.length > 0) {
            const command = params.argv.join(' ');
            await this.executeCommand(params.processId, command);
        }

        return { processId: params.processId };
    }

    /**
     * Execute a command in a session and buffer the output.
     */
    private async executeCommand(processId: string, command: string): Promise<void> {
        const session = this.sessions.get(processId);
        if (!session) return;

        try {
            const result = await mcpToolCall('pty_exec', {
                process_id: processId,
                command,
            }) as { content?: { type: string; text: string }[] } | undefined;

            let output = '';
            if (result?.content) {
                output = result.content
                    .filter((c) => c.type === 'text')
                    .map((c) => c.text)
                    .join('\n');
            }

            const exitMatch = output.match(/\[exit_code:\s*(-?\d+)\]\s*$/);
            if (exitMatch) {
                session.exitCode = parseInt(exitMatch[1], 10);
                output = output.slice(0, exitMatch.index).trimEnd();
            }

            if (output) {
                const encoder = new TextEncoder();
                session.chunks.push({
                    seq: session.nextSeq,
                    data: encoder.encode(output),
                });
                session.nextSeq++;
            }

            session.wakeSeq++;
            for (const resolve of session.wakeResolvers.splice(0)) {
                resolve();
            }
        } catch (err) {
            const errMsg = err instanceof Error ? err.message : String(err);

            const exitMatch = errMsg.match(/\[exit_code:\s*(-?\d+)\]/);
            if (exitMatch) {
                session.exitCode = parseInt(exitMatch[1], 10);
            }

            const encoder = new TextEncoder();
            session.chunks.push({
                seq: session.nextSeq,
                data: encoder.encode(errMsg.replace(/\[exit_code:\s*-?\d+\]\s*$/, '').trimEnd()),
            });
            session.nextSeq++;
            session.wakeSeq++;
            for (const resolve of session.wakeResolvers.splice(0)) {
                resolve();
            }
        }
    }

    async read(
        processId: string,
        afterSeq: bigint | undefined,
        _maxBytes: number | undefined,
        waitMs: bigint | undefined,
    ): Promise<PtyReadResult> {
        const session = this.sessions.get(processId);
        if (!session) {
            return {
                chunks: [],
                nextSeq: 0n,
                exited: true,
                exitCode: undefined,
                closed: true,
                failure: 'unknown process',
            };
        }

        const minSeq = afterSeq ?? 0n;

        const collectChunks = () => {
            const newChunks = session.chunks.filter(c => c.seq >= minSeq);
            if (newChunks.length > 0) {
                const maxDelivered = newChunks[newChunks.length - 1].seq;
                session.chunks = session.chunks.filter(c => c.seq > maxDelivered);
            }
            return newChunks;
        };

        let chunks = collectChunks();

        if (chunks.length === 0 && waitMs && waitMs > 0n) {
            const waitTime = Math.min(Number(waitMs), 500);
            await new Promise<void>(resolve => {
                const timer = setTimeout(resolve, waitTime);
                session.wakeResolvers.push(() => { clearTimeout(timer); resolve(); });
            });
            chunks = collectChunks();
        }

        return {
            chunks,
            nextSeq: session.nextSeq,
            exited: session.exited,
            exitCode: session.exitCode,
            closed: session.exited,
            failure: undefined,
        };
    }

    async write(processId: string, data: Uint8Array): Promise<PtyWriteResult> {
        const session = this.sessions.get(processId);
        if (!session) {
            return { status: 'unknown-process' };
        }
        if (session.exited) {
            return { status: 'stdin-closed' };
        }

        const command = new TextDecoder().decode(data).trim();
        if (command) {
            await this.executeCommand(processId, command);
        }

        return { status: 'accepted' };
    }

    async terminate(processId: string): Promise<void> {
        console.log('[PTY] terminate:', processId);

        const session = this.sessions.get(processId);
        if (session) {
            session.exited = true;
            session.exitCode = session.exitCode ?? 0;
            session.wakeSeq = BigInt('18446744073709551615'); // u64::MAX sentinel
            for (const resolve of session.wakeResolvers.splice(0)) {
                resolve();
            }
        }

        try {
            await mcpToolCall('pty_terminate', { process_id: processId });
        } catch {
            // Best-effort cleanup
        }

        this.sessions.delete(processId);
    }

    async pollWake(processId: string): Promise<bigint> {
        const session = this.sessions.get(processId);
        if (!session) {
            return BigInt('18446744073709551615'); // u64::MAX = process gone
        }

        if (session.exited) {
            return BigInt('18446744073709551615');
        }

        await new Promise<void>(resolve => {
            const timer = setTimeout(resolve, 200);
            session.wakeResolvers.push(() => { clearTimeout(timer); resolve(); });
        });
        return session.wakeSeq;
    }
}

// ==========================================================================
// Init Handler
// ==========================================================================

async function handleInit(origin: string): Promise<void> {
    // Set environment
    setEnvironment([
        ['HOME', '/'],
        ['CODEX_HOME', '/.codex'],
        ['TERM', 'xterm-256color'],
        ['RUST_BACKTRACE', '1'],
        ['CODEX_EXEC_SERVER_URL', 'wasm-host'],
        ['CODEX_ORIGIN', origin],
    ]);

    // Initialize OPFS (createSyncAccessHandle works in Workers!)
    await initFilesystem();

    // Pre-create /.codex
    try {
        const root = await navigator.storage.getDirectory();
        await root.getDirectoryHandle('.codex', { create: true });
    } catch (e) {
        console.warn('[AppServerWorker] Failed to pre-create .codex:', e);
    }

    // Register PTY handler
    setPtyHandler(new PtySessionManager());

    // Start WASM (no requestAnimationFrame in Workers -- direct await)
    const exitCode: number = await (start() as unknown as Promise<number>);
    if (exitCode !== 0) {
        throw new Error(`App server start() returned exit code: ${exitCode}`);
    }

    self.postMessage({ type: 'started' });
}

// ==========================================================================
// Send Request / Notification Handlers
// ==========================================================================

async function handleSendRequest(callId: string, json: string): Promise<void> {
    try {
        const result: string = await (protocol.sendRequest(json) as unknown as Promise<string>);
        self.postMessage({ type: 'request-result', callId, json: result });
    } catch (err) {
        self.postMessage({ type: 'request-error', callId, message: String(err) });
    }
}

async function handleSendNotification(json: string): Promise<void> {
    try {
        await (protocol.sendNotification(json) as unknown as Promise<void>);
    } catch (err) {
        console.error('[AppServerWorker] sendNotification error:', err);
    }
}

// ==========================================================================
// Message Handler (Safari-safe addEventListener pattern)
// ==========================================================================

self.addEventListener('message', (e: MessageEvent) => {
    const msg = e.data;
    switch (msg.type) {
        case 'init':
            handleInit(msg.origin).catch((err: unknown) => {
                self.postMessage({ type: 'start-error', message: String(err) });
            });
            break;
        case 'send-request':
            handleSendRequest(msg.callId, msg.json);
            break;
        case 'send-notification':
            handleSendNotification(msg.json);
            break;
        case 'respond-to-server-request':
            protocol.respondToServerRequest(msg.requestId, msg.resultJson);
            break;
        case 'fail-server-request':
            protocol.failServerRequest(msg.requestId, msg.errorJson);
            break;
        case 'push-auth-callback':
            pushAuthCallback(msg.method, msg.path, msg.headers, new Uint8Array(msg.body));
            break;
        case 'shutdown':
            protocol.shutdown();
            break;
        // Responses to our requests
        case 'transport-response':
        case 'exec-response':
        case 'mcp-response':
        case 'approval-decision':
        case 'network-approval-decision': {
            const pending = pendingCalls.get(msg.callId);
            if (pending) {
                pendingCalls.delete(msg.callId);
                pending.resolve(msg);
            }
            break;
        }
    }
});

// ==========================================================================
// Ready Signal
// ==========================================================================

self.postMessage({ type: 'ready' });
