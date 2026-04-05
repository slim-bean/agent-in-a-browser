/**
 * WASM Worker for TUI Execution
 *
 * Hosts the WASM runtime in a dedicated Web Worker. Supports two modes:
 *
 * 1. Sync mode (Safari/non-JSPI): Uses Atomics.wait() for synchronous blocking
 *    on async operations (stdin, HTTP, etc.)
 *
 * 2. JSPI mode (Chrome/Firefox): Uses JSPI async suspension. WASM suspends
 *    and yields to the event loop when awaiting stdin, HTTP, etc.
 *    This mode also fixes OPFS (createSyncAccessHandle requires Worker context).
 *
 * NOTE: This file lives in frontend because it imports from frontend modules.
 * The WorkerBridge in wasi-shims accepts a worker URL parameter.
 */

import { initStdinSyncBridge } from '@tjfontaine/wasi-shims/stdin-sync-bridge.js';
import {
    STDIN_CONTROL,
    HTTP_CONTROL,
    BUFFER_LAYOUT,
    type WorkerMessage,
    type WorkerRunMessage,
} from '@tjfontaine/wasi-shims/worker-constants.js';

// Re-export for type compatibility
export { STDIN_CONTROL, HTTP_CONTROL, BUFFER_LAYOUT } from '@tjfontaine/wasi-shims/worker-constants.js';
export type {
    WorkerInitMessage,
    WorkerRunMessage,
    WorkerInputMessage,
    WorkerHttpResponseMessage,
    WorkerHttpHeadersMessage,
    WorkerResizeMessage,
    WorkerMessage,
} from '@tjfontaine/wasi-shims/worker-constants.js';

// Buffer layout derived from constants
const CONTROL_SIZE = BUFFER_LAYOUT.CONTROL_SIZE;
const STDIN_BUFFER_OFFSET = BUFFER_LAYOUT.STDIN_BUFFER_OFFSET;
const STDIN_BUFFER_SIZE = BUFFER_LAYOUT.STDIN_BUFFER_SIZE;
const HTTP_BUFFER_OFFSET = BUFFER_LAYOUT.HTTP_BUFFER_OFFSET;
const HTTP_BUFFER_SIZE = BUFFER_LAYOUT.HTTP_BUFFER_SIZE;

// ============================================================
// DEBUG INSTRUMENTATION
// ============================================================

import {
    createWorkerDebugState,
    initWorkerDebug,
    buildProbeResponse,
    type WorkerDebugState,
} from '../debug/wasm-debug.js';

/** Worker-side debug state for import tracing and probe responses */
const workerDebugState: WorkerDebugState = createWorkerDebugState();

// ============================================================
// STATE
// ============================================================

let controlArray: Int32Array | null = null;
let stdinDataArray: Uint8Array | null = null;
let httpDataArray: Uint8Array | null = null;
let opfsSharedBuffer: SharedArrayBuffer | null = null;
let initialized = false;

// Tracks which execution mode the worker is using (set on 'run' message)
let jspiMode = false;

// JSPI mode: reference to pushStdinData/setTerminalSize from ghostty-cli-shim
// These are set lazily when the JSPI TUI is started
let jspiPushStdinData: ((data: Uint8Array) => void) | null = null;
let jspiSetTerminalSize: ((cols: number, rows: number) => void) | null = null;

// Reference to codex-tui's pushAuthCallback for routing OAuth callbacks
// Set when the codex-tui module is loaded (either directly or via lazy loading)
let pushAuthCallback: ((method: string, path: string, headers: [string, string][], body: Uint8Array) => void) | null = null;

// JSPI mode: pending HTTP response resolvers for async transport
// Maps request ID to resolve/reject callbacks
let nextHttpRequestId = 1;
const pendingHttpRequests = new Map<number, {
    resolve: (value: { status: number; headers: [string, string][]; body: Uint8Array }) => void;
    reject: (reason: Error) => void;
    // For collecting streamed chunks
    chunks: Uint8Array[];
    status: number;
    headers: [string, string][];
}>();

// Pending HTTP response headers (sent via postMessage, stored here for streaming)
let pendingHttpHeaders: { status: number; headers: [string, string][] } | null = null;

// Helper function to read pendingHttpHeaders without TypeScript control flow analysis
// This prevents TS from narrowing the value to 'never' after setting it to null
function getPendingHttpHeaders(): { status: number; headers: [string, string][] } | null {
    return pendingHttpHeaders;
}

// ============================================================
// INITIALIZATION
// ============================================================

/**
 * Initialize the worker with shared memory from main thread.
 * @param buffer SharedArrayBuffer for stdin/http communication
 * @param opfsBuffer SharedArrayBuffer for OPFS worker communication (required for WebKit)
 */
function initWorker(buffer: SharedArrayBuffer, opfsBuffer: SharedArrayBuffer): void {
    // Store buffers for stdin/http communication
    controlArray = new Int32Array(buffer, 0, CONTROL_SIZE / 4);
    stdinDataArray = new Uint8Array(buffer, STDIN_BUFFER_OFFSET, STDIN_BUFFER_SIZE);
    httpDataArray = new Uint8Array(buffer, HTTP_BUFFER_OFFSET, HTTP_BUFFER_SIZE);

    // Store OPFS buffer for filesystem initialization
    // In WebKit workers, SharedArrayBuffer is not available, so it must be passed from main thread
    opfsSharedBuffer = opfsBuffer;

    // Clear control flags
    Atomics.store(controlArray, STDIN_CONTROL.REQUEST_READY, 0);
    Atomics.store(controlArray, STDIN_CONTROL.RESPONSE_READY, 0);
    Atomics.store(controlArray, STDIN_CONTROL.EOF, 0);
    Atomics.store(controlArray, HTTP_CONTROL.REQUEST_READY, 0);
    Atomics.store(controlArray, HTTP_CONTROL.RESPONSE_READY, 0);

    // Initialize stdin sync bridge so ghostty-cli-shim knows we're in worker mode
    initStdinSyncBridge(controlArray, stdinDataArray);

    initialized = true;
    console.log('[WasmWorker] Initialized with SharedArrayBuffer');

    // Notify main thread we're ready
    self.postMessage({ type: 'ready' });
}

// ============================================================
// SYNC MODE: BLOCKING OPERATIONS (called from shims)
// ============================================================

/**
 * Synchronously read stdin data.
 * Blocks via Atomics.wait() until main thread provides input.
 */
export function blockingReadStdin(maxLen: number): Uint8Array {
    if (!controlArray || !stdinDataArray) {
        throw new Error('Worker not initialized');
    }

    // Check for EOF
    if (Atomics.load(controlArray, STDIN_CONTROL.EOF) === 1) {
        return new Uint8Array(0);
    }

    // Signal we want input
    Atomics.store(controlArray, STDIN_CONTROL.REQUEST_READY, 1);

    // Notify main thread (in case it's waiting)
    self.postMessage({ type: 'stdin-request', maxLen });

    // Block until input is available (30 second timeout)
    const waitResult = Atomics.wait(controlArray, STDIN_CONTROL.RESPONSE_READY, 0, 30000);

    if (waitResult === 'timed-out') {
        console.warn('[WasmWorker] stdin read timed out');
        return new Uint8Array(0);
    }

    // Read the data
    const dataLen = Atomics.load(controlArray, STDIN_CONTROL.DATA_LENGTH);
    const data = stdinDataArray.slice(0, Math.min(dataLen, maxLen));

    // Reset flags
    Atomics.store(controlArray, STDIN_CONTROL.RESPONSE_READY, 0);
    Atomics.store(controlArray, STDIN_CONTROL.REQUEST_READY, 0);

    return data;
}

/**
 * Synchronously perform HTTP request.
 * Blocks via Atomics.wait() until main thread completes fetch.
 */
export function blockingHttpRequest(
    method: string,
    url: string,
    headers: Record<string, string>,
    body: Uint8Array | null
): { status: number; headers: [string, string][]; body: Uint8Array } {
    if (!controlArray || !httpDataArray) {
        throw new Error('Worker not initialized');
    }

    // Send request to main thread
    self.postMessage({
        type: 'http-request',
        method,
        url,
        headers,
        body: body ? Array.from(body) : null
    });

    // Signal request is ready
    Atomics.store(controlArray, HTTP_CONTROL.REQUEST_READY, 1);

    // Block until response is available
    console.log('[WasmWorker] Waiting for HTTP response via Atomics.wait...');
    const currentValue = Atomics.load(controlArray, HTTP_CONTROL.RESPONSE_READY);
    console.log('[WasmWorker] RESPONSE_READY current value:', currentValue);

    const waitResult = Atomics.wait(controlArray, HTTP_CONTROL.RESPONSE_READY, 0, 60000);
    console.log('[WasmWorker] Atomics.wait returned:', waitResult);

    if (waitResult === 'timed-out') {
        console.error('[WasmWorker] HTTP request timed out');
        return { status: 0, headers: [], body: new Uint8Array(0) };
    }

    // Read response from shared buffer
    const status = Atomics.load(controlArray, HTTP_CONTROL.STATUS_CODE);
    const bodyLen = Atomics.load(controlArray, HTTP_CONTROL.BODY_LENGTH);
    console.log('[WasmWorker] HTTP response status:', status, 'bodyLen:', bodyLen);
    const responseBody = httpDataArray.slice(0, bodyLen);

    // Reset flags
    Atomics.store(controlArray, HTTP_CONTROL.RESPONSE_READY, 0);
    Atomics.store(controlArray, HTTP_CONTROL.REQUEST_READY, 0);

    // Headers are sent via postMessage, not SharedArrayBuffer
    // Main thread will have sent them before notifying
    return { status, headers: [], body: responseBody };
}

/**
 * Result type for streaming HTTP response chunks.
 */
export interface HttpStreamChunk {
    status: number;           // HTTP status (only valid on first chunk)
    headers: [string, string][]; // Response headers (only valid on first chunk)
    chunk: Uint8Array;        // Body chunk data
    done: boolean;            // True if this is the last chunk (EOF)
}

/**
 * Streaming HTTP request using a generator pattern.
 * Yields chunks as they arrive from the main thread.
 * Blocks via Atomics.wait() on each chunk.
 *
 * @param method HTTP method
 * @param url Request URL
 * @param headers Request headers
 * @param body Request body (optional)
 * @yields HttpStreamChunk for each chunk of response data
 */
export function* blockingHttpRequestStreaming(
    method: string,
    url: string,
    headers: Record<string, string>,
    body: Uint8Array | null
): Generator<HttpStreamChunk, void, unknown> {
    if (!controlArray || !httpDataArray) {
        throw new Error('Worker not initialized');
    }

    // Clear any pending headers from previous request
    pendingHttpHeaders = null;

    // Send request to main thread
    self.postMessage({
        type: 'http-request',
        method,
        url,
        headers,
        body: body ? Array.from(body) : null
    });

    // Signal request is ready
    Atomics.store(controlArray, HTTP_CONTROL.REQUEST_READY, 1);
    console.log('[WasmWorker] Streaming HTTP request started:', method, url);

    let isFirst = true;

    while (true) {
        // Wait for next chunk (or headers on first iteration)
        const waitResult = Atomics.wait(controlArray, HTTP_CONTROL.RESPONSE_READY, 0, 60000);

        if (waitResult === 'timed-out') {
            console.error('[WasmWorker] HTTP streaming timed out');
            Atomics.store(controlArray, HTTP_CONTROL.REQUEST_READY, 0);
            yield { status: 0, headers: [], chunk: new Uint8Array(0), done: true };
            return;
        }

        // Read chunk info from shared buffer
        const status = isFirst ? Atomics.load(controlArray, HTTP_CONTROL.STATUS_CODE) : 0;
        const chunkLen = Atomics.load(controlArray, HTTP_CONTROL.BODY_LENGTH);
        const isDone = Atomics.load(controlArray, HTTP_CONTROL.DONE) === 1;

        // Copy chunk data
        const chunk = httpDataArray.slice(0, chunkLen);

        // Get headers from pending (sent via postMessage) on first chunk
        // Use helper function to avoid TypeScript control flow narrowing to 'never'
        // (pendingHttpHeaders is set asynchronously by message handler)
        const pendingHeaders = getPendingHttpHeaders();
        const responseHeaders = isFirst && pendingHeaders ? pendingHeaders.headers : [];

        // Reset response ready flag
        Atomics.store(controlArray, HTTP_CONTROL.RESPONSE_READY, 0);

        // Signal we consumed this chunk (so main thread can send next)
        Atomics.store(controlArray, HTTP_CONTROL.CHUNK_CONSUMED, 1);
        Atomics.notify(controlArray, HTTP_CONTROL.CHUNK_CONSUMED);

        console.log(`[WasmWorker] Streaming chunk: ${chunkLen} bytes, done=${isDone}`);

        yield {
            status: isFirst ? (pendingHeaders?.status ?? status) : 0,
            headers: responseHeaders,
            chunk,
            done: isDone
        };

        if (isDone) {
            break;
        }

        isFirst = false;
    }

    // Clean up
    Atomics.store(controlArray, HTTP_CONTROL.REQUEST_READY, 0);
    pendingHttpHeaders = null;
    console.log('[WasmWorker] Streaming HTTP request complete');
}

// ============================================================
// JSPI MODE: ASYNC HTTP REQUEST (via postMessage round-trip)
// ============================================================

/**
 * Perform an async HTTP request by sending to main thread and awaiting response.
 * Used in JSPI mode where WASM suspends on the Promise (no Atomics.wait needed).
 */
function asyncHttpRequest(
    method: string,
    url: string,
    headers: Record<string, string>,
    body: Uint8Array | null
): Promise<{ status: number; headers: [string, Uint8Array][]; body: Uint8Array }> {
    const requestId = nextHttpRequestId++;

    return new Promise((resolve, reject) => {
        pendingHttpRequests.set(requestId, {
            resolve: (result) => resolve({
                status: result.status,
                headers: result.headers.map(([k, v]) => [k, new TextEncoder().encode(v)] as [string, Uint8Array]),
                body: result.body,
            }),
            reject,
            chunks: [],
            status: 0,
            headers: [],
        });

        // Send request to main thread with our request ID
        self.postMessage({
            type: 'http-request',
            method,
            url,
            headers,
            body: body ? Array.from(body) : null,
            requestId,
        });
    });
}

/**
 * Handle an HTTP response chunk in JSPI mode.
 * Collects chunks and resolves the pending Promise when done.
 */
function handleJspiHttpResponse(
    status: number,
    bodyChunk: Uint8Array,
    done: boolean,
    requestId?: number,
): void {
    // Find the pending request - use requestId if provided, otherwise take the oldest
    let pending: typeof pendingHttpRequests extends Map<number, infer V> ? V : never;
    let key: number;

    if (requestId !== undefined && pendingHttpRequests.has(requestId)) {
        key = requestId;
        pending = pendingHttpRequests.get(requestId)!;
    } else {
        // Fallback: use the first (oldest) pending request
        const first = pendingHttpRequests.entries().next();
        if (first.done) {
            console.warn('[WasmWorker JSPI] Received HTTP response but no pending request');
            return;
        }
        [key, pending] = first.value;
    }

    if (status !== 0) {
        pending.status = status;
    }
    if (bodyChunk.length > 0) {
        pending.chunks.push(bodyChunk);
    }

    if (done) {
        // Combine all chunks
        const totalLen = pending.chunks.reduce((sum, c) => sum + c.length, 0);
        const combined = new Uint8Array(totalLen);
        let offset = 0;
        for (const chunk of pending.chunks) {
            combined.set(chunk, offset);
            offset += chunk.length;
        }

        pendingHttpRequests.delete(key);
        pending.resolve({
            status: pending.status,
            headers: pending.headers,
            body: combined,
        });
    }
}

/**
 * Handle HTTP headers in JSPI mode.
 */
function handleJspiHttpHeaders(
    status: number,
    headers: [string, string][],
): void {
    // Store headers on the oldest pending request
    const first = pendingHttpRequests.entries().next();
    if (!first.done) {
        const [, pending] = first.value;
        pending.status = status;
        pending.headers = headers;
    }
}

// ============================================================
// TUI RUNNERS
// ============================================================

/**
 * Run TUI in sync mode (Safari/non-JSPI).
 * Uses Atomics.wait() for blocking I/O.
 */
async function runTuiSync(msg: WorkerRunMessage): Promise<void> {
    console.log('[WasmWorker] Initializing OPFS filesystem for TUI (sync mode)...');

    // Load sync Codex TUI module (imports shims via package paths)
    console.log('[WasmWorker] Loading sync codex-tui module (with shims)...');
    const tuiModule = await import('../wasm/codex-tui-sync/codex-wasm-tui.js');

    // Import filesystem shim and initialize
    const opfsShim = await import('@tjfontaine/wasi-shims/opfs-filesystem-sync-impl.js');

    // DEBUG: Check if the Descriptor classes are the same
    const shimDescriptor = opfsShim.types?.Descriptor;
    const rootDirs = opfsShim.preopens?.getDirectories?.();
    const rootDesc = rootDirs?.[0]?.[0];
    console.log('[WasmWorker] DEBUG - Descriptor class name:', shimDescriptor?.name);
    console.log('[WasmWorker] DEBUG - rootDesc constructor:', rootDesc?.constructor?.name);
    console.log('[WasmWorker] DEBUG - rootDesc instanceof Descriptor:', rootDesc instanceof shimDescriptor);
    console.log('[WasmWorker] DEBUG - Same class?:', rootDesc?.constructor === shimDescriptor ? 'YES' : 'NO');

    // Initialize OPFS with buffer from main thread (required for WebKit)
    await opfsShim.initFilesystem(opfsSharedBuffer!);
    console.log('[WasmWorker] OPFS filesystem ready');

    // Pre-load all lazy modules in the worker context
    console.log('[WasmWorker] Pre-loading all lazy modules...');
    const { initializeForSyncMode } = await import('../wasm/lazy-loading/lazy-modules.js');
    await initializeForSyncMode();
    console.log('[WasmWorker] Lazy modules pre-loaded');

    // Set up sync transport handler for MCP requests
    const { setTransportHandler, setStreamingTransportHandler } = await import('@tjfontaine/wasi-shims/wasi-http-impl.js');

    // Legacy sync transport handler (for backwards compatibility)
    setTransportHandler((method, url, headers, body) => {
        const response = blockingHttpRequest(method, url, headers, body);
        return {
            syncValue: {
                status: response.status,
                headers: response.headers.map(([k, v]) => [k, new TextEncoder().encode(v)] as [string, Uint8Array]),
                body: response.body
            }
        };
    }, true); // isSyncMode = true

    // Streaming transport handler
    setStreamingTransportHandler(function* (method, url, headers, body) {
        const generator = blockingHttpRequestStreaming(method, url, headers, body);
        for (const chunk of generator) {
            yield {
                status: chunk.status,
                headers: chunk.headers.map(([k, v]) => [k, new TextEncoder().encode(v)] as [string, Uint8Array]),
                chunk: chunk.chunk,
                done: chunk.done
            };
        }
    });
    console.log('[WasmWorker] Sync MCP transport handler registered (with streaming support)');

    // Await $init for sync module initialization
    if (tuiModule.$init) {
        console.log('[WasmWorker] Awaiting TUI module $init...');
        await tuiModule.$init;
    }

    console.log('[WasmWorker] TUI module loaded, starting run()...');
    self.postMessage({ type: 'started', module: msg.module });

    // Run the TUI
    try {
        const exitCode = tuiModule.run();
        console.log('[WasmWorker] TUI exited with code:', exitCode);
        self.postMessage({ type: 'exit', code: exitCode });
    } catch (err) {
        console.error('[WasmWorker] TUI execution error:', err);
        self.postMessage({ type: 'error', message: String(err) });
    }
}

// ============================================================
// SHARED WATCHDOG — detects WASM hangs across all module paths
// ============================================================

/**
 * Start the WASM watchdog timer. Tracks yield activity, stderr output,
 * and stdin reads. When a stall is detected (no yields for 10s or no
 * stderr for 30s), dumps pending imports and live resources.
 *
 * Returns a cleanup function to call when the module exits.
 */
function startWasmWatchdog(debugState: { pending: Map<number, { module: string; name: string; startTime: number }> }): () => void {
    const watchdogState = {
        lastStderrTime: Date.now(),
        lastStdinTime: Date.now(),
        lastYieldTime: Date.now(),
        startTime: Date.now(),
    };

    // Hook postMessage to track stderr activity
    const origPostMessage = self.postMessage.bind(self);
    self.postMessage = function(msg: unknown, ...args: unknown[]) {
        if ((msg as { type?: string })?.type === 'terminal-output') {
            watchdogState.lastStderrTime = Date.now();
        }
        return (origPostMessage as (...a: unknown[]) => void)(msg, ...args);
    };

    // Wire yield hook for DurationPollable.block()
    (globalThis as Record<string, unknown>).__wasmWatchdogState = watchdogState;
    (globalThis as Record<string, unknown>).__wasmYieldActivity = () => {
        watchdogState.lastYieldTime = Date.now();
    };

    const interval = setInterval(() => {
        const now = Date.now();
        const uptimeSec = ((now - watchdogState.startTime) / 1000).toFixed(1);
        const sinceStderrSec = ((now - watchdogState.lastStderrTime) / 1000).toFixed(1);
        const sinceStdinSec = ((now - watchdogState.lastStdinTime) / 1000).toFixed(1);
        const sinceYieldSec = ((now - watchdogState.lastYieldTime) / 1000).toFixed(1);
        const yieldStalled = now - watchdogState.lastYieldTime > 10000;
        const stderrStalled = now - watchdogState.lastStderrTime > 30000;

        if (yieldStalled || stderrStalled) {
            const pendingList: string[] = [];
            for (const [, call] of debugState.pending) {
                const elapsed = ((performance.now() - call.startTime) / 1000).toFixed(1);
                pendingList.push(`${call.module}/${call.name} (${elapsed}s)`);
            }
            const pendingInfo = pendingList.length > 0
                ? `\n  Pending JSPI imports: ${pendingList.join(', ')}`
                : '\n  No pending JSPI imports (stuck in pure WASM or Mutex deadlock)';

            let resourceInfo = '';
            try {
                const registryKey = Symbol.for('wasi:debug/resource-registry');
                const registry = (globalThis as Record<symbol, unknown>)[registryKey] as
                    { snapshot?: () => Array<{ type: string; subtype: string; meta: Record<string, string> }> } | undefined;
                if (registry?.snapshot) {
                    const resources = registry.snapshot();
                    if (resources.length > 0) {
                        const summary = resources.map(r => {
                            const meta = Object.entries(r.meta).map(([k, v]) => `${k}=${v}`).join(',');
                            return `${r.type}:${r.subtype}${meta ? `(${meta})` : ''}`;
                        }).join(', ');
                        resourceInfo = `\n  Live resources (${resources.length}): ${summary}`;
                    }
                }
            } catch { /* registry not available */ }

            console.warn(
                `[WasmWorker WATCHDOG] STALL DETECTED ` +
                `(uptime=${uptimeSec}s, sinceYield=${sinceYieldSec}s, ` +
                `sinceStderr=${sinceStderrSec}s, sinceStdin=${sinceStdinSec}s)` +
                pendingInfo + resourceInfo
            );
        } else {
            console.log(
                `[WasmWorker WATCHDOG] alive: uptime=${uptimeSec}s, ` +
                `sinceYield=${sinceYieldSec}s, sinceStderr=${sinceStderrSec}s, sinceStdin=${sinceStdinSec}s`
            );
        }
    }, 15000);

    return () => clearInterval(interval);
}

/**
 * Run TUI in JSPI mode (Chrome/Firefox).
 * Uses JSPI async suspension for blocking I/O.
 * WASM suspends and yields to the worker event loop when awaiting async ops.
 */
async function runTuiJspi(msg: WorkerRunMessage): Promise<void> {
    console.log('[WasmWorker] Initializing TUI in JSPI mode...');
    jspiMode = true;

    // Import ghostty-cli-shim functions for feeding stdin from postMessage
    const cliShim = await import('@tjfontaine/wasi-shims/ghostty-cli-shim.js');
    jspiPushStdinData = cliShim.pushStdinData;
    jspiSetTerminalSize = cliShim.setTerminalSize;

    // Apply initial terminal size from the run message before any WASM reads it
    if (msg.cols && msg.rows) {
        cliShim.setTerminalSize(msg.cols, msg.rows);
        console.log(`[WasmWorker JSPI] Initial terminal size set: ${msg.cols}x${msg.rows}`);
    }

    // Set environment variables (same as tui-loader.ts)
    cliShim.setEnvironment([
        ['HOME', '/'],
        ['CODEX_HOME', '/.codex'],
        ['TERM', 'xterm-256color'],
        ['SHELL', '/bin/sh'],
        ['RUST_BACKTRACE', '1'],
        ['CODEX_ORIGIN', self.location.origin],
    ]);

    // Initialize OPFS filesystem (async version, works in Workers)
    console.log('[WasmWorker JSPI] Initializing OPFS filesystem...');
    const { initFilesystem } = await import('@tjfontaine/wasi-shims/opfs-filesystem-impl.js');
    await initFilesystem();
    console.log('[WasmWorker JSPI] OPFS filesystem ready');

    // Pre-create /.codex in OPFS
    try {
        const root = await navigator.storage.getDirectory();
        await root.getDirectoryHandle('.codex', { create: true });
        console.log('[WasmWorker JSPI] Pre-created /.codex in OPFS');
    } catch (e) {
        console.warn('[WasmWorker JSPI] Failed to pre-create .codex in OPFS:', e);
    }

    // Set up async transport handler — direct fetch() in Worker for API calls,
    // route MCP requests through main thread
    const { setTransportHandler } = await import('@tjfontaine/wasi-shims/wasi-http-impl.js');
    setTransportHandler(async (method: string, url: string, headers: Record<string, string>, body: Uint8Array | null) => {
        console.log('[WasmWorker JSPI] HTTP request:', method, url);
        const urlObj = new URL(url);
        const isMcp = urlObj.pathname.startsWith('/mcp/');

        if (isMcp) {
            // MCP requests route through main thread → SharedWorker
            return asyncHttpRequest(method, url, headers, body);
        }

        // Direct fetch() for all other requests (OpenAI API, etc.)
        const fetchHeaders = new Headers();
        for (const [k, v] of Object.entries(headers)) {
            fetchHeaders.set(k, v);
        }
        const fetchOpts: RequestInit = { method, headers: fetchHeaders };
        if (body && body.length > 0) {
            fetchOpts.body = body as BodyInit;
        }
        const response = await fetch(url, fetchOpts);
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
    });
    console.log('[WasmWorker JSPI] HTTP transport handler registered (direct fetch + MCP relay)');

    // Register shell exec handler (same as tui-loader.ts, routes through MCP)
    const { setExecHandler } = await import('@tjfontaine/wasi-shims/shell-exec-impl.js');
    setExecHandler(async (
        program: string,
        args: string[],
        env: { cwd?: string },
        stdin: Uint8Array | undefined,
        timeoutMs: number | undefined,
    ) => {
        const command = [program, ...args].join(' ');
        console.log('[WasmWorker JSPI] Shell exec:', command.slice(0, 100), 'cwd:', env.cwd);
        const encoder = new TextEncoder();

        try {
            const body = JSON.stringify({
                jsonrpc: '2.0',
                id: Date.now(),
                method: 'tools/call',
                params: {
                    name: 'shell_eval',
                    arguments: {
                        command,
                    },
                },
            });

            // Route through MCP via the async HTTP transport (goes to main thread sandbox)
            const response = await asyncHttpRequest(
                'POST',
                'http://localhost:3000/mcp/message',
                { 'Content-Type': 'application/json' },
                encoder.encode(body),
            );

            const resultText = new TextDecoder().decode(response.body);
            console.log('[WasmWorker JSPI] Shell exec MCP response:', resultText.slice(0, 500));
            const result = JSON.parse(resultText);

            if (result.error) {
                return {
                    exitCode: 1,
                    stdout: new Uint8Array(0),
                    stderr: encoder.encode(result.error.message || 'MCP error'),
                };
            }

            const content = result.result?.content ?? [];
            const text = content
                .filter((c: { type: string }) => c.type === 'text')
                .map((c: { text: string }) => c.text)
                .join('\n');

            return {
                exitCode: 0,
                stdout: encoder.encode(text),
                stderr: new Uint8Array(0),
            };
        } catch (err) {
            console.error('[WasmWorker JSPI] Shell exec error:', err);
            return {
                exitCode: 127,
                stdout: new Uint8Array(0),
                stderr: encoder.encode(`exec failed: ${err instanceof Error ? err.message : String(err)}`),
            };
        }
    });
    console.log('[WasmWorker JSPI] Shell exec handler registered');

    // Auto-wrap shims with debug tracing BEFORE loading the TUI module.
    // This ensures the JCO-transpiled module gets already-patched prototypes
    // when it imports the shims. Wrapping after import doesn't work because
    // Vite may bundle separate module instances.
    try {
        const wrappedCount = await initWorkerDebug(workerDebugState);
        console.log(`[WasmWorker JSPI] Debug instrumentation: ${wrappedCount} imports wrapped`);
    } catch (err) {
        console.warn('[WasmWorker JSPI] Debug instrumentation failed (non-fatal):', err);
    }

    // Load the JSPI Codex TUI module (after shims are wrapped)
    console.log('[WasmWorker JSPI] Loading JSPI codex-tui module...');
    const tuiModule = await import('../wasm/codex-tui/codex-wasm-tui.js');
    console.log('[WasmWorker JSPI] TUI module loaded');

    // Store pushAuthCallback reference for OAuth callback routing
    if (typeof tuiModule.pushAuthCallback === 'function') {
        pushAuthCallback = tuiModule.pushAuthCallback;
        console.log('[WasmWorker JSPI] pushAuthCallback registered');
    }

    self.postMessage({ type: 'started', module: msg.module });

    // Show loading indicator via stdout (routed to terminal via postMessage)
    self.postMessage({
        type: 'terminal-output',
        data: '\r\n  Loading Codex...\r\n'
    });

    // Register 'codex' as a lazy-loaded interactive command
    const { registerCodexTui } = await import('../wasm/lazy-loading/lazy-modules.js');
    registerCodexTui();

    // Start watchdog for hang detection (shared with shell path)
    const watchdogCleanup = startWasmWatchdog(workerDebugState);

    // Run the TUI (async via JSPI - returns a Promise)
    try {
        console.log('[WasmWorker JSPI] Calling run()...');
        const exitCode = await tuiModule.run();
        console.log('[WasmWorker JSPI] TUI exited with code:', exitCode);
        watchdogCleanup();
        self.postMessage({ type: 'exit', code: exitCode });
    } catch (err) {
        console.error('[WasmWorker JSPI] TUI execution error:', err);
        watchdogCleanup();
        self.postMessage({ type: 'error', message: String(err) });
    }
}

/**
 * Run the brush shell as the primary entry point in JSPI mode.
 * Loads ts-runtime-mcp.wasm and calls shell:unix/command::run("sh").
 * The shell supports lazy-loading interactive commands like `codex`, `vim`, etc.
 */
async function runShellJspi(msg: WorkerRunMessage): Promise<void> {
    console.log('[WasmWorker] Initializing Shell in JSPI mode...');
    jspiMode = true;

    // Same setup as TUI: cli-shim, environment, OPFS, HTTP transport, shell-exec
    const cliShim = await import('@tjfontaine/wasi-shims/ghostty-cli-shim.js');
    jspiPushStdinData = cliShim.pushStdinData;
    jspiSetTerminalSize = cliShim.setTerminalSize;

    // Apply initial terminal size from the run message before any WASM reads it
    if (msg.cols && msg.rows) {
        cliShim.setTerminalSize(msg.cols, msg.rows);
        console.log(`[WasmWorker Shell] Initial terminal size set: ${msg.cols}x${msg.rows}`);
    }

    cliShim.setEnvironment([
        ['HOME', '/'],
        ['CODEX_HOME', '/.codex'],
        ['TERM', 'xterm-256color'],
        ['SHELL', '/bin/sh'],
        ['PATH', '/usr/local/bin:/usr/bin:/bin'],
        ['CODEX_ORIGIN', self.location.origin],
    ]);

    // Initialize OPFS filesystem
    console.log('[WasmWorker Shell] Initializing OPFS filesystem...');
    const { initFilesystem } = await import('@tjfontaine/wasi-shims/opfs-filesystem-impl.js');
    await initFilesystem();
    console.log('[WasmWorker Shell] OPFS filesystem ready');

    // Pre-create home directories at OPFS root
    try {
        const root = await navigator.storage.getDirectory();
        await root.getDirectoryHandle('.codex', { create: true });
        await root.getDirectoryHandle('.config', { create: true });
    } catch (e) {
        console.warn('[WasmWorker Shell] Failed to pre-create directories:', e);
    }

    // Set up HTTP transport (for MCP relay and direct fetch)
    const { setTransportHandler } = await import('@tjfontaine/wasi-shims/wasi-http-impl.js');
    setTransportHandler(async (method: string, url: string, headers: Record<string, string>, body: Uint8Array | null) => {
        const urlObj = new URL(url);
        const isMcp = urlObj.pathname.startsWith('/mcp/');

        if (isMcp) {
            return asyncHttpRequest(method, url, headers, body);
        }

        const fetchHeaders = new Headers();
        for (const [k, v] of Object.entries(headers)) {
            fetchHeaders.set(k, v);
        }
        const fetchOpts: RequestInit = { method, headers: fetchHeaders };
        if (body && body.length > 0) {
            fetchOpts.body = body as BodyInit;
        }
        const response = await fetch(url, fetchOpts);
        const responseBody = new Uint8Array(await response.arrayBuffer());
        const responseHeaders: [string, Uint8Array][] = [];
        response.headers.forEach((value, name) => {
            responseHeaders.push([name.toLowerCase(), new TextEncoder().encode(value)]);
        });
        return { status: response.status, headers: responseHeaders, body: responseBody };
    });
    console.log('[WasmWorker Shell] HTTP transport registered');

    // Register shell exec handler (needed by Codex TUI when launched as lazy command)
    const { setExecHandler } = await import('@tjfontaine/wasi-shims/shell-exec-impl.js');
    setExecHandler(async (
        program: string,
        args: string[],
        env: { cwd?: string },
        stdin: Uint8Array | undefined,
        _timeoutMs: number | undefined,
    ) => {
        const command = [program, ...args].join(' ');
        console.log('[WasmWorker Shell] Shell exec:', command.slice(0, 100), 'cwd:', env.cwd);
        const encoder = new TextEncoder();

        try {
            const body = JSON.stringify({
                jsonrpc: '2.0',
                id: Date.now(),
                method: 'tools/call',
                params: { name: 'shell_eval', arguments: { command } },
            });

            const response = await asyncHttpRequest(
                'POST',
                'http://localhost:3000/mcp/message',
                { 'Content-Type': 'application/json' },
                encoder.encode(body),
            );

            const resultText = new TextDecoder().decode(response.body);
            const result = JSON.parse(resultText);

            if (result.error) {
                return {
                    exitCode: 1,
                    stdout: new Uint8Array(0),
                    stderr: encoder.encode(result.error.message || 'MCP error'),
                };
            }

            const content = result.result?.content ?? [];
            const text = content
                .filter((c: { type: string }) => c.type === 'text')
                .map((c: { text: string }) => c.text)
                .join('\n');

            return {
                exitCode: 0,
                stdout: encoder.encode(text),
                stderr: new Uint8Array(0),
            };
        } catch (err) {
            console.error('[WasmWorker Shell] Shell exec error:', err);
            return {
                exitCode: 127,
                stdout: new Uint8Array(0),
                stderr: encoder.encode(`exec failed: ${err instanceof Error ? err.message : String(err)}`),
            };
        }
    });
    console.log('[WasmWorker Shell] Shell exec handler registered');

    // Register lazy modules (codex, vim, tsx, sqlite3, etc.)
    const { registerAllModules, registerCodexTui } = await import('../wasm/lazy-loading/lazy-modules.js');
    registerAllModules();
    registerCodexTui();

    // Load the MCP module (which exports shell:unix/command)
    console.log('[WasmWorker Shell] Loading ts-runtime-mcp module...');
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const mcpModule: any = await import('@tjfontaine/mcp-wasm-server/mcp-server-jspi/ts-runtime-mcp.js');
    if (mcpModule.$init) {
        await mcpModule.$init;
    }
    console.log('[WasmWorker Shell] Module loaded');

    self.postMessage({ type: 'started', module: msg.module });

    // Start watchdog for hang detection (shared with TUI path)
    const watchdogCleanup = startWasmWatchdog(workerDebugState);

    // Run the shell REPL
    try {
        console.log('[WasmWorker Shell] Starting brush shell REPL...');
        const exitCode = await mcpModule.command.run(
            'sh',
            [],
            { cwd: '/', vars: [] },
            cliShim.stdin.getStdin(),
            cliShim.stdout.getStdout(),
            cliShim.stderr.getStderr(),
        );
        console.log('[WasmWorker Shell] Shell exited with code:', exitCode);
        watchdogCleanup();
        self.postMessage({ type: 'exit', code: exitCode });
    } catch (err) {
        console.error('[WasmWorker Shell] Shell execution error:', err);
        watchdogCleanup();
        self.postMessage({ type: 'error', message: String(err) });
    }
}

// ============================================================
// MESSAGE HANDLER
// ============================================================

self.onmessage = async (event: MessageEvent<WorkerMessage>) => {
    const msg = event.data;

    // Handle debug messages (not part of the WorkerMessage union type)
    const msgAny = msg as any;
    if (msgAny.type === 'debug-probe') {
        self.postMessage(buildProbeResponse(workerDebugState));
        return;
    }
    if (msgAny.type === 'debug-trace') {
        workerDebugState.traceEnabled = !!msgAny.enable;
        console.log(`[WasmWorker] Import tracing ${workerDebugState.traceEnabled ? 'ENABLED' : 'DISABLED'}`);
        return;
    }
    if (msgAny.type === 'debug-resource-dump') {
        // Dynamically access the resource registry singleton
        let resources: unknown[] = [];
        try {
            const registryKey = Symbol.for('wasi:debug/resource-registry');
            const registry = (globalThis as Record<symbol, unknown>)[registryKey] as
                { snapshot?: () => unknown[] } | undefined;
            if (registry && typeof registry.snapshot === 'function') {
                resources = registry.snapshot();
            }
        } catch {
            // Registry may not be loaded yet
        }
        self.postMessage({ type: 'debug-resource-dump-response', resources });
        return;
    }
    if (msgAny.type === 'debug-wrap-shims') {
        initWorkerDebug(workerDebugState).then(count => {
            self.postMessage({ type: 'debug-wrap-response', wrappedCount: count });
        });
        return;
    }

    // OAuth callback routing — push the callback request into the codex-tui's
    // tiny_http channel so the login server's recv() loop picks it up.
    if (msgAny.type === 'oauth-callback') {
        const handler = pushAuthCallback
            ?? (globalThis as Record<string, unknown>).__pushAuthCallback as typeof pushAuthCallback;
        if (handler) {
            // Use /auth/callback to match the upstream server.rs path matching
            const path = `/auth/callback?code=${encodeURIComponent(msgAny.code)}&state=${encodeURIComponent(msgAny.state)}`;
            console.log('[WasmWorker] Routing OAuth callback to pushAuthCallback:', path);
            handler('GET', path, [], new Uint8Array(0));
        } else {
            console.warn('[WasmWorker] OAuth callback received but pushAuthCallback not available');
        }
        return;
    }

    switch (msg.type) {
        case 'init':
            initWorker(msg.sharedBuffer, msg.opfsSharedBuffer);
            break;

        case 'stdin':
            // Track stdin activity for watchdog
            if ((globalThis as any).__wasmWatchdogState) {
                (globalThis as any).__wasmWatchdogState.lastStdinTime = Date.now();
            }
            // Main thread is providing stdin data
            if (jspiMode && jspiPushStdinData && msg.data) {
                // JSPI mode: push into ghostty-cli-shim's async stdin buffer
                jspiPushStdinData(msg.data instanceof Uint8Array ? msg.data : new Uint8Array(msg.data));
            } else if (controlArray && stdinDataArray && msg.data) {
                // Sync mode: write to SharedArrayBuffer and wake via Atomics
                const data = msg.data instanceof Uint8Array ? msg.data : new Uint8Array(msg.data);
                stdinDataArray.set(data);
                Atomics.store(controlArray, STDIN_CONTROL.DATA_LENGTH, data.length);
                Atomics.store(controlArray, STDIN_CONTROL.RESPONSE_READY, 1);
                Atomics.notify(controlArray, STDIN_CONTROL.RESPONSE_READY);
            }
            break;

        case 'resize':
            // Update cached terminal size in ghostty-cli-shim.
            // The crossterm shim detects size changes via WIT terminal:info/size.
            // No escape sequences injected into stdin.
            if (jspiSetTerminalSize && msg.cols && msg.rows) {
                jspiSetTerminalSize(msg.cols, msg.rows);
            }
            break;

        case 'http-response':
            if (jspiMode) {
                // JSPI mode: resolve pending async HTTP request
                handleJspiHttpResponse(
                    msg.status,
                    msg.bodyChunk,
                    msg.done,
                    (msg as any).requestId,
                );
            } else if (controlArray && httpDataArray) {
                // Sync mode: write to SharedArrayBuffer and wake via Atomics
                httpDataArray.set(msg.bodyChunk);
                Atomics.store(controlArray, HTTP_CONTROL.STATUS_CODE, msg.status);
                Atomics.store(controlArray, HTTP_CONTROL.BODY_LENGTH, msg.bodyChunk.length);
                Atomics.store(controlArray, HTTP_CONTROL.DONE, msg.done ? 1 : 0);
                Atomics.store(controlArray, HTTP_CONTROL.RESPONSE_READY, 1);
                Atomics.notify(controlArray, HTTP_CONTROL.RESPONSE_READY);
            }
            break;

        case 'http-headers':
            if (jspiMode) {
                // JSPI mode: store headers on pending request
                handleJspiHttpHeaders(msg.status, msg.headers);
            } else {
                // Sync mode: store for streaming generator
                pendingHttpHeaders = {
                    status: msg.status,
                    headers: msg.headers
                };
                if (controlArray) {
                    Atomics.store(controlArray, HTTP_CONTROL.HEADERS_READY, 1);
                    Atomics.notify(controlArray, HTTP_CONTROL.HEADERS_READY);
                }
            }
            break;

        case 'run':
            // Start WASM execution
            if (!initialized) {
                self.postMessage({ type: 'error', message: 'Worker not initialized' });
                return;
            }

            try {
                if (msg.module === 'shell' && msg.jspi) {
                    // ========================================================
                    // SHELL MODE (JSPI): Brush shell as primary entry point
                    // ========================================================
                    await runShellJspi(msg);
                } else if (msg.module === 'tui' && msg.jspi) {
                    // ========================================================
                    // JSPI MODE: Load async WASM module, use Promise-based I/O
                    // ========================================================
                    await runTuiJspi(msg);
                } else if (msg.module === 'tui') {
                    // ========================================================
                    // SYNC MODE: Load sync WASM module, use Atomics-based I/O
                    // ========================================================
                    await runTuiSync(msg);
                } else {
                    console.log(`[WasmWorker] Unknown module: ${msg.module}`);
                    self.postMessage({ type: 'error', message: `Unknown module: ${msg.module}` });
                }
            } catch (err) {
                console.error('[WasmWorker] Module load error:', err);
                self.postMessage({ type: 'error', message: String(err) });
            }
            break;
    }
};

// Export for type checking
