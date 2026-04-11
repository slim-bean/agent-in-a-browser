/**
 * App Server Loader - Connects frontend to codex-wasm-app-server WASM
 *
 * Launches the WASM in a dedicated Worker for native OPFS sync access.
 * The main thread proxies sandbox (MCP server) communication and handles
 * UI callbacks (events, approvals).
 */

// Type-only import for ApprovalDecision
import type { ApprovalDecision } from '@tjfontaine/wasi-shims/shell-exec-impl.js';
export type { ApprovalDecision };

// Sandbox for MCP routing
import { fetchFromSandbox, initializeSandbox } from '../../agent/sandbox.js';

// ==========================================================================
// Public Types
// ==========================================================================

export interface AppServerEvent {
    type: 'notification' | 'request' | 'lagged' | 'error';
    data?: unknown;
    skipped?: number;
    message?: string;
}

// ==========================================================================
// Worker Message Types (strongly typed Worker↔Main protocol)
// ==========================================================================

/** Events emitted by the WASM via emit_event, parsed from JSON. */
type WasmEvent =
    | { type: 'response'; id: string; result?: unknown; error?: { code: number; message: string } }
    | { type: 'started' }
    | AppServerEvent;

/** Messages from the Worker to the main thread. */
type WorkerMessage =
    | { type: 'ready' }
    | { type: 'started' }
    | { type: 'start-error'; message: string }
    | { type: 'event'; json: string }
    | { type: 'transport-request'; callId: string; method: string; url: string; headers: Record<string, string>; body: ArrayBuffer | null }
    | { type: 'exec-request'; callId: string; program: string; args: string[]; cwd: string; stdin: ArrayBuffer | null; timeoutMs: number }
    | { type: 'mcp-request'; callId: string; path: string; method: string; headers: Record<string, string>; body: string }
    | { type: 'approval-request'; callId: string; program: string; args: string[]; cwd: string }
    | { type: 'network-approval-request'; callId: string; url: string; method: string };

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
// Approval Callback Types and State
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
let eventHandler: ((event: AppServerEvent) => void) | null = null;

// ==========================================================================
// Call ID Infrastructure
// ==========================================================================

let callIdCounter = 0;
const pendingCalls = new Map<string, { resolve: (value: unknown) => void; reject: (reason: unknown) => void }>();

// ==========================================================================
// Proxy Helpers
// ==========================================================================

async function handleTransportProxy(
    worker: Worker,
    msg: Extract<WorkerMessage, { type: 'transport-request' }>,
): Promise<void> {
    const { callId, method, url, headers, body } = msg;

    try {
        const urlObj = new URL(url);
        const path = urlObj.pathname;
        console.log('[App Server Transport] Routing to sandbox:', method, path);

        const fetchOptions: RequestInit = { method, headers };
        if (body) {
            fetchOptions.body = new Blob([body]);
        }

        const response = await fetchFromSandbox(path, fetchOptions);
        const responseBody = new Uint8Array(await response.arrayBuffer());
        const responseHeaders: [string, Uint8Array][] = [];
        response.headers.forEach((value, name) => {
            responseHeaders.push([name.toLowerCase(), new TextEncoder().encode(value)]);
        });

        worker.postMessage(
            { type: 'transport-response', callId, status: response.status, headers: responseHeaders, body: responseBody },
            [responseBody.buffer],
        );
    } catch (err) {
        worker.postMessage({
            type: 'transport-response', callId,
            status: 502, headers: [], body: new TextEncoder().encode(String(err)),
        });
    }
}

async function handleExecProxy(
    worker: Worker,
    msg: Extract<WorkerMessage, { type: 'exec-request' }>,
): Promise<void> {
    const { callId, program, args, cwd, stdin, timeoutMs } = msg;
    const command = [program, ...args].join(' ');
    const encoder = new TextEncoder();

    try {
        const body = JSON.stringify({
            jsonrpc: '2.0', id: Date.now(),
            method: 'tools/call',
            params: {
                name: 'run_command',
                arguments: {
                    command,
                    cwd: cwd || '/workspace',
                    stdin: stdin ? new TextDecoder().decode(stdin) : undefined,
                    timeout_ms: timeoutMs ?? 30000,
                },
            },
        });

        const response = await fetchFromSandbox('/mcp/message', {
            method: 'POST', headers: { 'Content-Type': 'application/json' }, body,
        });
        const result: { error?: { message?: string }; result?: { content?: { type: string; text: string }[] } } = await response.json();

        if (result.error) {
            const stderr = encoder.encode(result.error.message ?? 'MCP error');
            worker.postMessage({ type: 'exec-response', callId, exitCode: 1, stdout: new Uint8Array(0), stderr }, [stderr.buffer]);
            return;
        }

        const text = (result.result?.content ?? []).filter(c => c.type === 'text').map(c => c.text).join('\n');
        const stdout = encoder.encode(text);
        worker.postMessage({ type: 'exec-response', callId, exitCode: 0, stdout, stderr: new Uint8Array(0) }, [stdout.buffer]);
    } catch (err) {
        const stderr = encoder.encode(`exec failed: ${err instanceof Error ? err.message : String(err)}`);
        worker.postMessage({ type: 'exec-response', callId, exitCode: 127, stdout: new Uint8Array(0), stderr }, [stderr.buffer]);
    }
}

async function handleMcpProxy(
    worker: Worker,
    msg: Extract<WorkerMessage, { type: 'mcp-request' }>,
): Promise<void> {
    const { callId, path, method, headers, body } = msg;

    try {
        const response = await fetchFromSandbox(path, { method, headers, body });
        const text = await response.text();
        worker.postMessage({ type: 'mcp-response', callId, status: response.status, body: text });
    } catch (err) {
        worker.postMessage({
            type: 'mcp-response', callId,
            status: 502,
            body: JSON.stringify({ error: { message: String(err) } }),
        });
    }
}

// ==========================================================================
// Main Entry Point
// ==========================================================================

/**
 * Launch the app-server WASM module in a dedicated Worker and return a typed
 * protocol handle.
 *
 * The Worker runs the WASM component directly (with OPFS sync access).
 * The main thread proxies sandbox HTTP, shell exec, and MCP requests,
 * and handles UI callbacks (events, approvals).
 */
export async function launchAppServer(options?: { origin?: string }): Promise<AppServerHandle> {
    // ----------------------------------------------------------------
    // 1. Initialize sandbox worker
    // ----------------------------------------------------------------
    console.log('[App Server] Initializing sandbox...');
    await initializeSandbox();
    console.log('[App Server] Sandbox ready');

    // ----------------------------------------------------------------
    // 2. Create dedicated Worker for WASM
    // ----------------------------------------------------------------
    console.log('[App Server] Creating WASM worker...');
    const worker = new Worker(
        new URL('../../workers/AppServerWorker.ts', import.meta.url),
        { type: 'module' },
    );

    // ----------------------------------------------------------------
    // 3. Wait for Worker ready signal
    // ----------------------------------------------------------------
    await new Promise<void>((resolve, reject) => {
        const onMessage = (e: MessageEvent) => {
            if (e.data.type === 'ready') {
                worker.removeEventListener('message', onMessage);
                resolve();
            }
        };
        worker.addEventListener('message', onMessage);
        worker.addEventListener('error', (e) => reject(new Error(`Worker error: ${e.message}`)));
    });
    console.log('[App Server] Worker ready');

    // ----------------------------------------------------------------
    // 4. Set up persistent message handler for all Worker messages
    // ----------------------------------------------------------------
    worker.addEventListener('message', (e: MessageEvent<WorkerMessage>) => {
        const msg = e.data;
        switch (msg.type) {
            // Events from WASM (notifications, requests, responses, errors)
            case 'event': {
                try {
                    const event = JSON.parse(msg.json) as WasmEvent;

                    // Protocol responses from inbox — correlate by id
                    if (event.type === 'response') {
                        const pending = pendingCalls.get(event.id);
                        if (pending) {
                            pendingCalls.delete(event.id);
                            if (event.error) {
                                pending.resolve(JSON.stringify({ error: event.error }));
                            } else {
                                pending.resolve(JSON.stringify({ result: event.result }));
                            }
                        }
                        break;
                    }

                    // 'started' events are handled by the init listener, not here
                    if (event.type === 'started') break;

                    // All other events → forward to UI event handler
                    if (eventHandler) {
                        eventHandler(event);
                    }
                } catch (err) {
                    if (eventHandler) {
                        eventHandler({
                            type: 'error',
                            message: `Failed to parse server event: ${err instanceof Error ? err.message : String(err)}`,
                        });
                    }
                }
                break;
            }

            // Transport proxy: Worker needs HTTP via sandbox
            case 'transport-request': {
                handleTransportProxy(worker, msg);
                break;
            }

            // Exec proxy: Worker needs shell exec via sandbox
            case 'exec-request': {
                handleExecProxy(worker, msg);
                break;
            }

            // MCP proxy: Worker needs MCP tool call via sandbox (for PTY)
            case 'mcp-request': {
                handleMcpProxy(worker, msg);
                break;
            }

            // Command approval: Worker needs UI decision
            case 'approval-request': {
                if (commandApprovalCallback) {
                    commandApprovalCallback(msg.program, msg.args, msg.cwd, (decision) => {
                        worker.postMessage({ type: 'approval-decision', callId: msg.callId, decision });
                    });
                } else {
                    worker.postMessage({ type: 'approval-decision', callId: msg.callId, decision: 'deny' });
                }
                break;
            }

            // Network approval: Worker needs UI decision
            case 'network-approval-request': {
                if (networkApprovalCallback) {
                    networkApprovalCallback(msg.url, msg.method, (decision) => {
                        worker.postMessage({ type: 'network-approval-decision', callId: msg.callId, decision });
                    });
                } else {
                    worker.postMessage({ type: 'network-approval-decision', callId: msg.callId, decision: 'deny' });
                }
                break;
            }

            case 'start-error': {
                console.error('[App Server] Worker start failed:', msg.message);
                break;
            }
        }
    });

    // ----------------------------------------------------------------
    // 5. Send init and wait for started
    // ----------------------------------------------------------------
    const origin = options?.origin ?? globalThis.location?.origin ?? 'https://agent.edge-agent.dev';

    await new Promise<void>((resolve, reject) => {
        const onStarted = (e: MessageEvent) => {
            if (e.data.type === 'started') {
                worker.removeEventListener('message', onStarted);
                resolve();
            } else if (e.data.type === 'start-error') {
                worker.removeEventListener('message', onStarted);
                reject(new Error(e.data.message as string));
            }
        };
        worker.addEventListener('message', onStarted);
        worker.postMessage({ type: 'init', origin });
    });

    console.log('[App Server] WASM runtime started');

    // ----------------------------------------------------------------
    // 6. Return AppServerHandle wrapping Worker communication
    // ----------------------------------------------------------------
    const handle: AppServerHandle = {
        async sendRequest(json: string): Promise<string> {
            const callId = `req-${++callIdCounter}`;
            return new Promise<string>((resolve, reject) => {
                pendingCalls.set(callId, {
                    resolve: resolve as (v: unknown) => void,
                    reject,
                });
                worker.postMessage({ type: 'send-request', callId, json });
            });
        },

        async sendNotification(json: string): Promise<void> {
            worker.postMessage({ type: 'send-notification', json });
        },

        async respondToServerRequest(requestId: string, resultJson: string): Promise<void> {
            worker.postMessage({ type: 'respond-to-server-request', requestId, resultJson });
        },

        async failServerRequest(requestId: string, errorJson: string): Promise<void> {
            worker.postMessage({ type: 'fail-server-request', requestId, errorJson });
        },

        async shutdown(): Promise<void> {
            worker.postMessage({ type: 'shutdown' });
            worker.terminate();
        },

        async pushAuthCallback(
            method: string,
            path: string,
            headers: [string, string][],
            body: Uint8Array,
        ): Promise<void> {
            const buffer = body.buffer;
            worker.postMessage(
                { type: 'push-auth-callback', method, path, headers, body: buffer },
                [buffer],
            );
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
