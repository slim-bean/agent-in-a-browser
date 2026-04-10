/**
 * App Server Loader - Connects frontend to codex-wasm-app-server WASM
 *
 * This module provides the bridge between a custom frontend UI and the
 * Codex app-server running as a WASM component. Unlike the TUI loader,
 * this does NOT create a terminal — it exposes a typed protocol handle
 * for JSON-based client-to-server communication and an event callback
 * for server-to-client push events.
 */

// Import the Codex App Server WASM module (transpiled with jco)
import {
    start,
    protocol,
    pushAuthCallback,
} from '../codex-app-server/codex-wasm-app-server.js';

// Import the CLI shim to set up environment variables
import { setEnvironment } from '@tjfontaine/wasi-shims/ghostty-cli-shim.js';

// Import transport handler for routing HTTP requests (LLM API calls, etc.)
import { setTransportHandler, setNetworkApprovalHandler } from '@tjfontaine/wasi-shims/wasi-http-impl.js';

// Import shell exec handler registration for command execution
import {
    setExecHandler,
    setApprovalHandler,
    type ExecEnv,
    type ExecResult,
    type ApprovalDecision,
} from '@tjfontaine/wasi-shims/shell-exec-impl.js';

// Re-export for consumers of AppServerHandle
export type { ApprovalDecision };

// Import PTY handler registration for persistent shell sessions
import {
    setPtyHandler,
    type PtyHandler,
    type PtyStartParams,
    type PtyStartResult,
    type PtyReadResult,
    type PtyWriteResult,
} from '@tjfontaine/wasi-shims/shell-pty-impl.js';

// Import event sink handler registration for server push events
import { setEventHandler } from '@tjfontaine/wasi-shims/event-sink-impl.js';

// Import sandbox for MCP routing and shell execution
import { fetchFromSandbox, initializeSandbox } from '../../agent/sandbox.js';

// Import OPFS filesystem init for shell access
import { initFilesystem } from '@tjfontaine/wasi-shims/opfs-filesystem-impl.js';

// ==========================================================================
// Approval Callback Types
// ==========================================================================

type CommandApprovalCallback = (
    program: string,
    args: string[],
    cwd: string,
    respond: (decision: ApprovalDecision) => void,
) => void;

type NetworkApprovalCallback = (
    url: string,
    method: string,
    respond: (decision: ApprovalDecision) => void,
) => void;

let commandApprovalCallback: CommandApprovalCallback | null = null;
let networkApprovalCallback: NetworkApprovalCallback | null = null;

// ==========================================================================
// Public Types
// ==========================================================================

export interface AppServerEvent {
    type: 'notification' | 'request' | 'lagged' | 'error';
    data?: unknown;
    skipped?: number;
    message?: string;
}

export interface AppServerHandle {
    sendRequest(json: string): Promise<string>;
    sendNotification(json: string): Promise<void>;
    respondToServerRequest(requestId: string, resultJson: string): Promise<void>;
    failServerRequest(requestId: string, errorJson: string): Promise<void>;
    shutdown(): Promise<void>;
    pushAuthCallback(method: string, path: string, headers: [string, string][], body: Uint8Array): Promise<void>;
    onEvent(handler: (event: AppServerEvent) => void): void;

    onCommandApproval(handler: (
        program: string,
        args: string[],
        cwd: string,
        respond: (decision: ApprovalDecision) => void,
    ) => void): void;

    onNetworkApproval(handler: (
        url: string,
        method: string,
        respond: (decision: ApprovalDecision) => void,
    ) => void): void;
}

// ==========================================================================
// Sandbox Transport (same as TUI loader)
// ==========================================================================

/**
 * Create a transport handler that routes HTTP requests through the sandbox worker
 */
function createSandboxTransport() {
    return async (
        method: string,
        url: string,
        headers: Record<string, string>,
        body: Uint8Array | null
    ): Promise<{ status: number; headers: [string, Uint8Array][]; body: Uint8Array }> => {
        const urlObj = new URL(url);
        const path = urlObj.pathname;

        console.log('[App Server Transport] Routing to sandbox:', method, path);

        const fetchOptions: RequestInit = {
            method,
            headers,
        };

        if (body) {
            fetchOptions.body = new Blob([body as BlobPart]);
        }

        const response = await fetchFromSandbox(path, fetchOptions);

        const responseBody = new Uint8Array(await response.arrayBuffer());
        const responseHeaders: [string, Uint8Array][] = [];
        response.headers.forEach((value, name) => {
            responseHeaders.push([name.toLowerCase(), new TextEncoder().encode(value)]);
        });

        return {
            status: response.status,
            headers: responseHeaders,
            body: responseBody,
        };
    };
}

// ==========================================================================
// PTY Session Manager (same as TUI loader)
// ==========================================================================

/**
 * PTY Session Manager - routes persistent shell sessions through the MCP server.
 *
 * Each session holds a ShellEnv on the MCP server side (via pty_start/pty_exec/pty_terminate
 * MCP tools). Output is buffered with sequence numbers for incremental reads.
 */
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

    /** Call MCP tool via the sandbox. */
    private async callTool(name: string, args: Record<string, unknown>): Promise<unknown> {
        const body = JSON.stringify({
            jsonrpc: '2.0',
            id: Date.now(),
            method: 'tools/call',
            params: { name, arguments: args },
        });
        const response = await fetchFromSandbox('/mcp/message', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body,
        });
        const json: { error?: { message?: string }; result?: unknown } = await response.json();
        if (json.error) {
            throw new Error(json.error.message ?? `${name} failed`);
        }
        return json.result;
    }

    async start(params: PtyStartParams): Promise<PtyStartResult> {
        console.log('[PTY] start:', params.processId, 'argv:', params.argv, 'cwd:', params.cwd);

        await this.callTool('pty_start', {
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
            const result = await this.callTool('pty_exec', {
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
            await this.callTool('pty_terminate', { process_id: processId });
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

        const currentWake = session.wakeSeq;
        void currentWake; // suppress unused warning
        await new Promise<void>(resolve => {
            const timer = setTimeout(resolve, 200);
            session.wakeResolvers.push(() => { clearTimeout(timer); resolve(); });
        });
        return session.wakeSeq;
    }
}

// ==========================================================================
// Main Entry Point
// ==========================================================================

/**
 * Launch the app-server WASM module and return a typed protocol handle.
 *
 * Unlike launchTui(), this does NOT create a terminal. The caller provides
 * its own UI and communicates with the agent via the returned handle.
 */
export async function launchAppServer(options?: { origin?: string }): Promise<AppServerHandle> {
    // ----------------------------------------------------------------
    // 1. Initialize sandbox worker (same as TUI loader)
    // ----------------------------------------------------------------
    console.log('[App Server] Initializing sandbox...');
    await initializeSandbox();
    console.log('[App Server] Sandbox ready');

    // ----------------------------------------------------------------
    // 2. Register transport handler (same as TUI loader)
    // ----------------------------------------------------------------
    setTransportHandler(createSandboxTransport());
    console.log('[App Server] Transport handler configured');

    // ----------------------------------------------------------------
    // 3. Register exec handler (same as TUI loader)
    // ----------------------------------------------------------------
    setExecHandler(async (
        program: string,
        args: string[],
        env: ExecEnv,
        stdin: Uint8Array | undefined,
        timeoutMs: number | undefined,
    ): Promise<ExecResult> => {
        const command = [program, ...args].join(' ');
        console.log('[App Server] Shell exec:', command, 'cwd:', env.cwd);
        const encoder = new TextEncoder();

        try {
            const body = JSON.stringify({
                jsonrpc: '2.0',
                id: Date.now(),
                method: 'tools/call',
                params: {
                    name: 'run_command',
                    arguments: {
                        command,
                        cwd: env.cwd || '/workspace',
                        stdin: stdin ? new TextDecoder().decode(stdin) : undefined,
                        timeout_ms: timeoutMs ?? 30000,
                    },
                },
            });

            const response = await fetchFromSandbox('/mcp/message', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body,
            });

            const result: {
                error?: { message?: string };
                result?: { content?: { type: string; text: string }[] };
            } = await response.json();

            if (result.error) {
                return {
                    exitCode: 1,
                    stdout: new Uint8Array(0),
                    stderr: encoder.encode(result.error.message ?? 'MCP error'),
                };
            }

            const content = result.result?.content ?? [];
            const text = content
                .filter((c) => c.type === 'text')
                .map((c) => c.text)
                .join('\n');

            return {
                exitCode: 0,
                stdout: encoder.encode(text),
                stderr: new Uint8Array(0),
            };
        } catch (err) {
            console.error('[App Server] Shell exec error:', err);
            return {
                exitCode: 127,
                stdout: new Uint8Array(0),
                stderr: encoder.encode(`exec failed: ${err instanceof Error ? err.message : String(err)}`),
            };
        }
    });
    console.log('[App Server] Shell exec handler registered');

    // ----------------------------------------------------------------
    // 4. Register PTY handler (same as TUI loader)
    // ----------------------------------------------------------------
    setPtyHandler(new PtySessionManager());
    console.log('[App Server] PTY session handler registered');

    // ----------------------------------------------------------------
    // 5. Initialize OPFS filesystem
    // ----------------------------------------------------------------
    console.log('[App Server] Initializing OPFS filesystem...');
    await initFilesystem();
    console.log('[App Server] OPFS filesystem ready');

    // ----------------------------------------------------------------
    // 6. Set environment variables
    // ----------------------------------------------------------------
    const origin = options?.origin ?? globalThis.location?.origin ?? 'https://agent.edge-agent.dev';

    setEnvironment([
        ['HOME', '/'],
        ['CODEX_HOME', '/.codex'],
        ['TERM', 'xterm-256color'],
        ['RUST_BACKTRACE', '1'],
        ['CODEX_EXEC_SERVER_URL', 'wasm-host'],
        ['CODEX_ORIGIN', origin],
    ]);

    // Pre-create /.codex in OPFS so find_codex_home() succeeds.
    try {
        const root = await navigator.storage.getDirectory();
        await root.getDirectoryHandle('.codex', { create: true });
        console.log('[App Server] Pre-created /.codex in OPFS');
    } catch (e) {
        console.warn('[App Server] Failed to pre-create .codex in OPFS:', e);
    }

    // ----------------------------------------------------------------
    // 7. Register policy approval handlers
    // ----------------------------------------------------------------
    setApprovalHandler(async (program, args, env) => {
        return new Promise<ApprovalDecision>((resolve) => {
            if (commandApprovalCallback) {
                commandApprovalCallback(program, args, env.cwd, (decision) => {
                    resolve(decision);
                });
            } else {
                // No UI handler registered = deny by default
                resolve('deny');
            }
        });
    });

    setNetworkApprovalHandler(async (url, method) => {
        return new Promise<ApprovalDecision>((resolve) => {
            if (networkApprovalCallback) {
                networkApprovalCallback(url, method, (decision) => {
                    resolve(decision);
                });
            } else {
                // No UI handler registered = deny by default
                resolve('deny');
            }
        });
    });
    console.log('[App Server] Policy approval handlers registered');

    // ----------------------------------------------------------------
    // 8. Register event handler
    // ----------------------------------------------------------------
    let eventHandler: ((event: AppServerEvent) => void) | null = null;

    setEventHandler((json: string) => {
        if (!eventHandler) return;
        try {
            const event = JSON.parse(json) as AppServerEvent;
            eventHandler(event);
        } catch (err) {
            console.error('[App Server] Failed to parse event:', err, json);
            eventHandler({
                type: 'error',
                message: `Failed to parse server event: ${err instanceof Error ? err.message : String(err)}`,
            });
        }
    });
    console.log('[App Server] Event handler registered');

    // ----------------------------------------------------------------
    // 9. Call start() from WASM
    // ----------------------------------------------------------------
    console.log('[App Server] Calling start()...');

    // start() initializes the app-server runtime (async via JSPI).
    // Use requestAnimationFrame to ensure any pending UI work flushes first.
    await new Promise<void>((resolve, reject) => {
        requestAnimationFrame(() => {
            start().then((exitCode: number) => {
                if (exitCode !== 0) {
                    reject(new Error(`App server start() returned exit code: ${exitCode}`));
                } else {
                    resolve();
                }
            }).catch((err: unknown) => {
                reject(err);
            });
        });
    });

    console.log('[App Server] WASM runtime started');

    // ----------------------------------------------------------------
    // 10. Return AppServerHandle wrapping protocol exports
    // ----------------------------------------------------------------
    const handle: AppServerHandle = {
        async sendRequest(json: string): Promise<string> {
            // protocol.sendRequest returns string synchronously per .d.ts,
            // but JSPI wraps it as Promise<string> at runtime. Use await
            // to handle both cases correctly.
            return await (protocol.sendRequest(json) as unknown as Promise<string>);
        },

        async sendNotification(json: string): Promise<void> {
            await (protocol.sendNotification(json) as unknown as Promise<void>);
        },

        async respondToServerRequest(requestId: string, resultJson: string): Promise<void> {
            await (protocol.respondToServerRequest(requestId, resultJson) as unknown as Promise<void>);
        },

        async failServerRequest(requestId: string, errorJson: string): Promise<void> {
            await (protocol.failServerRequest(requestId, errorJson) as unknown as Promise<void>);
        },

        async shutdown(): Promise<void> {
            await (protocol.shutdown() as unknown as Promise<void>);
            setTransportHandler(null); // Clean up transport handler
        },

        async pushAuthCallback(
            method: string,
            path: string,
            headers: [string, string][],
            body: Uint8Array,
        ): Promise<void> {
            await pushAuthCallback(method, path, headers, body);
        },

        onEvent(handler: (event: AppServerEvent) => void): void {
            eventHandler = handler;
        },

        onCommandApproval(handler: (
            program: string,
            args: string[],
            cwd: string,
            respond: (decision: ApprovalDecision) => void,
        ) => void): void {
            commandApprovalCallback = handler;
        },

        onNetworkApproval(handler: (
            url: string,
            method: string,
            respond: (decision: ApprovalDecision) => void,
        ) => void): void {
            networkApprovalCallback = handler;
        },
    };

    return handle;
}
