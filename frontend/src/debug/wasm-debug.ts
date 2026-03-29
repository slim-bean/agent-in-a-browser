/**
 * WASM Debug Utilities
 *
 * Provides runtime instrumentation for diagnosing WASM/JSPI hangs in the
 * Codex TUI. Exposes `window.__wasmDebug` with tools for:
 *
 *   - Import tracing: wraps every WASI/WIT import with call/return logging
 *   - Pending import tracking: shows which JSPI-suspended imports have not returned
 *   - Worker probing: sends debug messages to the Worker and reports state
 *
 * Usage from Chrome DevTools console:
 *
 *   __wasmDebug.status()           // overview of WASM state
 *   __wasmDebug.pendingImports()   // list imports that called but haven't returned
 *   __wasmDebug.traceImports(true) // enable verbose import call logging
 *   __wasmDebug.probeWorker()      // ask the Worker to report its internal state
 *   __wasmDebug.importHistory(20)  // last N import calls with timing
 *   __wasmDebug.hangDetector(5000) // set hang threshold in ms (default 5000)
 */

// ============================================================
// TYPES
// ============================================================

interface ImportCall {
    id: number;
    module: string;
    name: string;
    startTime: number;
    endTime: number | null;
    duration: number | null;
    status: 'pending' | 'resolved' | 'rejected';
    isAsync: boolean;
    error?: string;
    /** Abbreviated stringified args (first 200 chars each) */
    args?: string[];
}

interface DebugState {
    /** Whether verbose import tracing is enabled */
    traceEnabled: boolean;
    /** Milliseconds before a pending import triggers a console warning */
    hangThresholdMs: number;
    /** Monotonically increasing call counter */
    nextCallId: number;
    /** Currently pending (unresolved) import calls, keyed by callId */
    pending: Map<number, ImportCall>;
    /** Circular buffer of recent import calls (resolved or not) */
    history: ImportCall[];
    /** Max history entries to keep */
    historySize: number;
    /** When the debug system was initialized */
    initTime: number;
    /** Reference to the Worker (set from main-tui.ts) */
    worker: Worker | null;
    /** Interval ID for the hang detector */
    hangDetectorInterval: ReturnType<typeof setInterval> | null;
    /** Responses from worker probes */
    lastProbeResponse: unknown;
}

// ============================================================
// STATE
// ============================================================

const state: DebugState = {
    traceEnabled: false,
    hangThresholdMs: 5000,
    nextCallId: 0,
    pending: new Map(),
    history: [],
    historySize: 500,
    initTime: Date.now(),
    worker: null,
    hangDetectorInterval: null,
    lastProbeResponse: null,
};

// ============================================================
// IMPORT TRACING
// ============================================================

function abbreviate(val: unknown, maxLen = 200): string {
    try {
        if (val === undefined) return 'undefined';
        if (val === null) return 'null';
        if (val instanceof Uint8Array) return `Uint8Array(${val.length})`;
        if (val instanceof ArrayBuffer) return `ArrayBuffer(${val.byteLength})`;
        if (typeof val === 'function') return `[Function: ${val.name || 'anonymous'}]`;
        const s = JSON.stringify(val);
        return s.length > maxLen ? s.slice(0, maxLen) + '...' : s;
    } catch {
        return String(val).slice(0, maxLen);
    }
}

function recordCall(module: string, name: string, args: unknown[]): ImportCall {
    const call: ImportCall = {
        id: state.nextCallId++,
        module,
        name,
        startTime: performance.now(),
        endTime: null,
        duration: null,
        status: 'pending',
        isAsync: false,
        args: args.map(a => abbreviate(a)),
    };
    state.pending.set(call.id, call);
    pushHistory(call);
    return call;
}

function resolveCall(call: ImportCall, error?: string): void {
    call.endTime = performance.now();
    call.duration = call.endTime - call.startTime;
    call.status = error ? 'rejected' : 'resolved';
    if (error) call.error = error;
    state.pending.delete(call.id);
}

function pushHistory(call: ImportCall): void {
    state.history.push(call);
    if (state.history.length > state.historySize) {
        state.history.shift();
    }
}

/**
 * Wraps a single import function with tracing. Works for both sync functions
 * and functions that return Promises (JSPI async imports).
 */
function wrapImportFn(module: string, name: string, fn: Function): Function {
    return function (this: unknown, ...args: unknown[]) {
        const call = recordCall(module, name, args);

        if (state.traceEnabled) {
            console.log(
                `%c[WASM Import] %c${module}/${name}%c called`,
                'color: #888', 'color: #4fc3f7; font-weight: bold', 'color: #888',
                call.args,
            );
        }

        let result: unknown;
        try {
            result = fn.apply(this, args);
        } catch (err) {
            resolveCall(call, String(err));
            if (state.traceEnabled) {
                console.error(
                    `[WASM Import] ${module}/${name} threw:`, err,
                );
            }
            throw err;
        }

        // Check if the result is a Promise (async JSPI import)
        if (result && typeof (result as Promise<unknown>).then === 'function') {
            call.isAsync = true;
            (result as Promise<unknown>).then(
                (val) => {
                    resolveCall(call);
                    if (state.traceEnabled) {
                        console.log(
                            `%c[WASM Import] %c${module}/${name}%c resolved (${call.duration?.toFixed(1)}ms)`,
                            'color: #888', 'color: #81c784; font-weight: bold', 'color: #888',
                        );
                    }
                    return val;
                },
                (err) => {
                    resolveCall(call, String(err));
                    if (state.traceEnabled) {
                        console.error(
                            `[WASM Import] ${module}/${name} rejected (${call.duration?.toFixed(1)}ms):`,
                            err,
                        );
                    }
                    throw err;
                },
            );
        } else {
            resolveCall(call);
            if (state.traceEnabled) {
                console.log(
                    `%c[WASM Import] %c${module}/${name}%c returned (${call.duration?.toFixed(1)}ms)`,
                    'color: #888', 'color: #81c784; font-weight: bold', 'color: #888',
                );
            }
        }

        return result;
    };
}

/**
 * Wraps all methods on an import object (or class prototype) with tracing.
 * Handles both plain objects of functions and ES6 class instances.
 */
function wrapImportObject(moduleName: string, obj: Record<string, unknown>): Record<string, unknown> {
    const wrapped: Record<string, unknown> = {};
    for (const key of Object.keys(obj)) {
        const val = obj[key];
        if (typeof val === 'function') {
            wrapped[key] = wrapImportFn(moduleName, key, val);
        } else if (val && typeof val === 'object' && !Array.isArray(val)) {
            // Nested namespace (e.g. types.Descriptor)
            // Wrap the prototype methods if it's a class constructor
            const ctor = val as { prototype?: Record<string, unknown> };
            if (ctor.prototype && typeof ctor === 'function') {
                wrapClassPrototype(moduleName, key, ctor as new (...args: unknown[]) => unknown);
            }
            wrapped[key] = val;
        } else {
            wrapped[key] = val;
        }
    }
    return wrapped;
}

/**
 * Patches prototype methods of a class used as a WASM import resource
 * (e.g. Pollable, InputStream, OutputStream, Descriptor, etc.)
 */
function wrapClassPrototype(moduleName: string, className: string, ctor: new (...args: unknown[]) => unknown): void {
    const proto = ctor.prototype;
    if (!proto) return;
    const descriptors = Object.getOwnPropertyDescriptors(proto);
    for (const [methodName, descriptor] of Object.entries(descriptors)) {
        if (methodName === 'constructor') continue;
        if (typeof descriptor.value === 'function') {
            const original = descriptor.value;
            Object.defineProperty(proto, methodName, {
                ...descriptor,
                value: wrapImportFn(moduleName, `${className}.${methodName}`, original),
            });
        }
    }
}

// ============================================================
// HANG DETECTOR
// ============================================================

function startHangDetector(): void {
    if (state.hangDetectorInterval) return;
    state.hangDetectorInterval = setInterval(() => {
        const now = performance.now();
        for (const [id, call] of state.pending) {
            const elapsed = now - call.startTime;
            if (elapsed > state.hangThresholdMs) {
                console.warn(
                    `%c[WASM HANG] %c${call.module}/${call.name}%c pending for ${(elapsed / 1000).toFixed(1)}s (call #${id})`,
                    'color: #f44336; font-weight: bold',
                    'color: #ff9800; font-weight: bold',
                    'color: #f44336',
                    { call },
                );
            }
        }
    }, 2000);
}

function stopHangDetector(): void {
    if (state.hangDetectorInterval) {
        clearInterval(state.hangDetectorInterval);
        state.hangDetectorInterval = null;
    }
}

// ============================================================
// SHIM WRAPPING ENTRY POINT
// ============================================================

/**
 * Map of shim module names to their import paths. These match the imports
 * at the top of the JCO-transpiled codex-wasm-tui.js.
 *
 * This function dynamically imports each shim and patches its exports in-place.
 * Call this BEFORE the WASM module is loaded (i.e. before `import('../wasm/codex-tui/...')`)
 * or call `wrapLoadedShims()` after load to patch already-imported modules.
 */
const SHIM_MODULES: Record<string, string> = {
    'wasi:clocks': '@tjfontaine/wasi-shims/clocks-impl.js',
    'wasi:io/error': '@tjfontaine/wasi-shims/error.js',
    'wasi:cli': '@tjfontaine/wasi-shims/ghostty-cli-shim.js',
    'wasi:filesystem': '@tjfontaine/wasi-shims/opfs-filesystem-impl.js',
    'wasi:io/poll': '@tjfontaine/wasi-shims/poll-impl.js',
    'wasi:random': '@tjfontaine/wasi-shims/random.js',
    'codex:tui/shell-exec': '@tjfontaine/wasi-shims/shell-exec-impl.js',
    'wasi:io/streams': '@tjfontaine/wasi-shims/streams.js',
    'wasi:http': '@tjfontaine/wasi-shims/wasi-http-impl.js',
};

/**
 * Wraps already-loaded shim modules by dynamically importing them and patching
 * their exported objects' prototypes. Since ES modules are singletons, patching
 * the prototype of an exported class affects all consumers.
 *
 * This is the recommended approach for the Worker context where imports happen
 * at module load time.
 */
export async function wrapLoadedShims(): Promise<number> {
    let wrappedCount = 0;

    for (const [moduleName, importPath] of Object.entries(SHIM_MODULES)) {
        try {
            const mod = await import(/* @vite-ignore */ importPath);
            for (const [exportName, exportVal] of Object.entries(mod)) {
                if (typeof exportVal === 'function') {
                    // Check if it's a class with prototype methods
                    const ctor = exportVal as { prototype?: Record<string, unknown> };
                    if (ctor.prototype && Object.getOwnPropertyNames(ctor.prototype).length > 1) {
                        wrapClassPrototype(moduleName, exportName, ctor as new (...args: unknown[]) => unknown);
                        wrappedCount++;
                    }
                } else if (exportVal && typeof exportVal === 'object') {
                    // It's a namespace object (e.g. monotonicClock, types, etc.)
                    const ns = exportVal as Record<string, unknown>;
                    for (const [key, val] of Object.entries(ns)) {
                        if (typeof val === 'function') {
                            // Wrap standalone functions by replacing on the namespace
                            ns[key] = wrapImportFn(moduleName, `${exportName}.${key}`, val);
                            wrappedCount++;
                        }
                    }
                }
            }
        } catch (err) {
            console.warn(`[WasmDebug] Failed to wrap shim ${moduleName}:`, err);
        }
    }

    console.log(`[WasmDebug] Wrapped ${wrappedCount} import functions/methods`);
    startHangDetector();
    return wrappedCount;
}

// ============================================================
// CONSOLE API  (window.__wasmDebug)
// ============================================================

export interface WasmDebugAPI {
    /** Overview of WASM debug state */
    status(): void;
    /** List all pending (unresolved) import calls */
    pendingImports(): ImportCall[];
    /** Toggle verbose import tracing */
    traceImports(enable: boolean): void;
    /** Send a debug probe to the Worker and print its response */
    probeWorker(): Promise<unknown>;
    /** Show last N import calls with timing */
    importHistory(n?: number): ImportCall[];
    /** Set the hang detection threshold in ms */
    hangDetector(thresholdMs?: number): void;
    /** Activate import wrapping from the main thread (for worker, use message) */
    wrapShims(): Promise<number>;
    /** Send a message to toggle tracing inside the Worker */
    workerTrace(enable: boolean): void;
    /** Access raw state for scripting */
    _state: DebugState;
}

function formatDuration(ms: number | null): string {
    if (ms === null) return 'pending';
    if (ms < 1) return `${(ms * 1000).toFixed(0)}us`;
    if (ms < 1000) return `${ms.toFixed(1)}ms`;
    return `${(ms / 1000).toFixed(2)}s`;
}

export function createDebugAPI(): WasmDebugAPI {
    const api: WasmDebugAPI = {
        status() {
            const uptimeSec = ((Date.now() - state.initTime) / 1000).toFixed(1);
            const pendingCount = state.pending.size;
            const totalCalls = state.nextCallId;
            const tracing = state.traceEnabled ? 'ON' : 'OFF';
            const hangDetector = state.hangDetectorInterval ? `ON (${state.hangThresholdMs}ms)` : 'OFF';
            const workerConnected = state.worker ? 'YES' : 'NO';

            console.log(
                `%c=== WASM Debug Status ===\n` +
                `%cUptime:          %c${uptimeSec}s\n` +
                `%cTotal calls:     %c${totalCalls}\n` +
                `%cPending imports: %c${pendingCount}%c${pendingCount > 0 ? ' (!!!)' : ''}\n` +
                `%cTracing:         %c${tracing}\n` +
                `%cHang detector:   %c${hangDetector}\n` +
                `%cWorker:          %c${workerConnected}`,
                'color: #4fc3f7; font-weight: bold; font-size: 14px',
                'color: #aaa', 'color: #fff',
                'color: #aaa', 'color: #fff',
                'color: #aaa', pendingCount > 0 ? 'color: #f44336; font-weight: bold' : 'color: #81c784', 'color: #f44336',
                'color: #aaa', state.traceEnabled ? 'color: #81c784' : 'color: #888',
                'color: #aaa', 'color: #fff',
                'color: #aaa', state.worker ? 'color: #81c784' : 'color: #888',
            );

            if (pendingCount > 0) {
                console.log('%cPending imports:', 'color: #ff9800; font-weight: bold');
                for (const [, call] of state.pending) {
                    const elapsed = performance.now() - call.startTime;
                    console.log(
                        `  #${call.id} ${call.module}/${call.name} - ${formatDuration(elapsed)} (async=${call.isAsync})`,
                    );
                }
            }
        },

        pendingImports(): ImportCall[] {
            const pending = Array.from(state.pending.values());
            if (pending.length === 0) {
                console.log('%c[WasmDebug] No pending imports', 'color: #81c784');
            } else {
                console.table(
                    pending.map(c => ({
                        id: c.id,
                        import: `${c.module}/${c.name}`,
                        elapsed: formatDuration(performance.now() - c.startTime),
                        async: c.isAsync,
                        args: c.args?.join(', ').slice(0, 100),
                    })),
                );
            }
            return pending;
        },

        traceImports(enable: boolean) {
            state.traceEnabled = enable;
            console.log(
                `%c[WasmDebug] Import tracing ${enable ? 'ENABLED' : 'DISABLED'}`,
                enable ? 'color: #81c784; font-weight: bold' : 'color: #888',
            );
            if (enable) {
                startHangDetector();
            }
        },

        async probeWorker(): Promise<unknown> {
            if (!state.worker) {
                console.error('[WasmDebug] No worker reference set. Call from main-tui.ts context.');
                return null;
            }

            return new Promise((resolve) => {
                const timeout = setTimeout(() => {
                    console.error('[WasmDebug] Worker probe timed out (5s). Worker may be blocked.');
                    resolve({ error: 'timeout' });
                }, 5000);

                const handler = (e: MessageEvent) => {
                    if (e.data?.type === 'debug-probe-response') {
                        clearTimeout(timeout);
                        state.worker?.removeEventListener('message', handler);
                        state.lastProbeResponse = e.data;
                        console.log('%c[WasmDebug] Worker probe response:', 'color: #4fc3f7; font-weight: bold');
                        console.log(e.data);
                        resolve(e.data);
                    }
                };

                state.worker!.addEventListener('message', handler);
                state.worker!.postMessage({ type: 'debug-probe' });
            });
        },

        importHistory(n = 30): ImportCall[] {
            const recent = state.history.slice(-n);
            console.table(
                recent.map(c => ({
                    id: c.id,
                    import: `${c.module}/${c.name}`,
                    duration: formatDuration(c.duration),
                    status: c.status,
                    async: c.isAsync,
                })),
            );
            return recent;
        },

        hangDetector(thresholdMs?: number) {
            if (thresholdMs !== undefined) {
                state.hangThresholdMs = thresholdMs;
                console.log(`[WasmDebug] Hang threshold set to ${thresholdMs}ms`);
            }
            if (!state.hangDetectorInterval) {
                startHangDetector();
                console.log('[WasmDebug] Hang detector started');
            } else {
                console.log(`[WasmDebug] Hang detector running (threshold: ${state.hangThresholdMs}ms)`);
            }
        },

        async wrapShims(): Promise<number> {
            const count = await wrapLoadedShims();
            return count;
        },

        workerTrace(enable: boolean) {
            if (!state.worker) {
                console.error('[WasmDebug] No worker reference set.');
                return;
            }
            state.worker.postMessage({ type: 'debug-trace', enable });
            console.log(`[WasmDebug] Sent trace ${enable ? 'enable' : 'disable'} to worker`);
        },

        _state: state,
    };

    return api;
}

// ============================================================
// WORKER-SIDE DEBUG STATE (used in wasm-worker.ts)
// ============================================================

/**
 * Worker-side debug state. Tracks import calls within the Worker context.
 * This is a separate instance from the main-thread state.
 */
export function createWorkerDebugState() {
    return {
        traceEnabled: false,
        hangThresholdMs: 5000,
        nextCallId: 0,
        pending: new Map<number, ImportCall>(),
        history: [] as ImportCall[],
        historySize: 500,
        initTime: Date.now(),
        hangDetectorInterval: null as ReturnType<typeof setInterval> | null,
    };
}

export type WorkerDebugState = ReturnType<typeof createWorkerDebugState>;

/**
 * Worker-side: wrap all loaded shims and start hang detection.
 * Returns the debug state for probe responses.
 */
export async function initWorkerDebug(debugState: WorkerDebugState): Promise<number> {
    let wrappedCount = 0;

    // In the Worker context, we patch the same shim modules
    for (const [moduleName, importPath] of Object.entries(SHIM_MODULES)) {
        try {
            const mod = await import(/* @vite-ignore */ importPath);
            for (const [exportName, exportVal] of Object.entries(mod)) {
                if (typeof exportVal === 'function') {
                    const ctor = exportVal as { prototype?: Record<string, unknown> };
                    if (ctor.prototype && Object.getOwnPropertyNames(ctor.prototype).length > 1) {
                        wrapClassPrototypeWorker(debugState, moduleName, exportName, ctor as new (...args: unknown[]) => unknown);
                        wrappedCount++;
                    }
                } else if (exportVal && typeof exportVal === 'object') {
                    const ns = exportVal as Record<string, unknown>;
                    for (const [key, val] of Object.entries(ns)) {
                        if (typeof val === 'function') {
                            ns[key] = wrapImportFnWorker(debugState, moduleName, `${exportName}.${key}`, val);
                            wrappedCount++;
                        }
                    }
                }
            }
        } catch (err) {
            console.warn(`[WasmDebug Worker] Failed to wrap shim ${moduleName}:`, err);
        }
    }

    // Start hang detector in Worker
    debugState.hangDetectorInterval = setInterval(() => {
        const now = performance.now();
        for (const [id, call] of debugState.pending) {
            const elapsed = now - call.startTime;
            if (elapsed > debugState.hangThresholdMs) {
                console.warn(
                    `[WASM HANG Worker] ${call.module}/${call.name} pending for ${(elapsed / 1000).toFixed(1)}s (call #${id})`,
                );
            }
        }
    }, 2000);

    console.log(`[WasmDebug Worker] Wrapped ${wrappedCount} import functions/methods`);
    return wrappedCount;
}

function wrapImportFnWorker(debugState: WorkerDebugState, module: string, name: string, fn: Function): Function {
    return function (this: unknown, ...args: unknown[]) {
        const call: ImportCall = {
            id: debugState.nextCallId++,
            module,
            name,
            startTime: performance.now(),
            endTime: null,
            duration: null,
            status: 'pending',
            isAsync: false,
            args: args.map(a => abbreviate(a)),
        };
        debugState.pending.set(call.id, call);
        debugState.history.push(call);
        if (debugState.history.length > debugState.historySize) {
            debugState.history.shift();
        }

        if (debugState.traceEnabled) {
            console.log(`[WASM Import Worker] ${module}/${name} called`, call.args);
        }

        let result: unknown;
        try {
            result = fn.apply(this, args);
        } catch (err) {
            call.endTime = performance.now();
            call.duration = call.endTime - call.startTime;
            call.status = 'rejected';
            call.error = String(err);
            debugState.pending.delete(call.id);
            throw err;
        }

        if (result && typeof (result as Promise<unknown>).then === 'function') {
            call.isAsync = true;
            (result as Promise<unknown>).then(
                (val) => {
                    call.endTime = performance.now();
                    call.duration = call.endTime - call.startTime;
                    call.status = 'resolved';
                    debugState.pending.delete(call.id);
                    if (debugState.traceEnabled) {
                        console.log(`[WASM Import Worker] ${module}/${name} resolved (${call.duration?.toFixed(1)}ms)`);
                    }
                    return val;
                },
                (err) => {
                    call.endTime = performance.now();
                    call.duration = call.endTime - call.startTime;
                    call.status = 'rejected';
                    call.error = String(err);
                    debugState.pending.delete(call.id);
                    throw err;
                },
            );
        } else {
            call.endTime = performance.now();
            call.duration = call.endTime - call.startTime;
            call.status = 'resolved';
            debugState.pending.delete(call.id);
            if (debugState.traceEnabled) {
                console.log(`[WASM Import Worker] ${module}/${name} returned (${call.duration?.toFixed(1)}ms)`);
            }
        }

        return result;
    };
}

function wrapClassPrototypeWorker(
    debugState: WorkerDebugState,
    moduleName: string,
    className: string,
    ctor: new (...args: unknown[]) => unknown,
): void {
    const proto = ctor.prototype;
    if (!proto) return;
    const descriptors = Object.getOwnPropertyDescriptors(proto);
    for (const [methodName, descriptor] of Object.entries(descriptors)) {
        if (methodName === 'constructor') continue;
        if (typeof descriptor.value === 'function') {
            const original = descriptor.value;
            Object.defineProperty(proto, methodName, {
                ...descriptor,
                value: wrapImportFnWorker(debugState, moduleName, `${className}.${methodName}`, original),
            });
        }
    }
}

/**
 * Build a probe response from Worker debug state.
 */
export function buildProbeResponse(debugState: WorkerDebugState): Record<string, unknown> {
    const pending = Array.from(debugState.pending.values()).map(c => ({
        id: c.id,
        import: `${c.module}/${c.name}`,
        elapsed: `${((performance.now() - c.startTime) / 1000).toFixed(1)}s`,
        async: c.isAsync,
        args: c.args?.join(', ').slice(0, 100),
    }));

    const recentHistory = debugState.history.slice(-20).map(c => ({
        id: c.id,
        import: `${c.module}/${c.name}`,
        duration: c.duration !== null ? `${c.duration.toFixed(1)}ms` : 'pending',
        status: c.status,
        async: c.isAsync,
    }));

    return {
        type: 'debug-probe-response',
        timestamp: Date.now(),
        uptime: `${((Date.now() - debugState.initTime) / 1000).toFixed(1)}s`,
        totalCalls: debugState.nextCallId,
        pendingCount: debugState.pending.size,
        pendingImports: pending,
        recentHistory,
        traceEnabled: debugState.traceEnabled,
    };
}

// ============================================================
// INSTALLATION
// ============================================================

/**
 * Install the debug API on window.__wasmDebug. Call from main-tui.ts.
 * Optionally pass the Worker reference for probe functionality.
 */
export function installDebugAPI(worker?: Worker): WasmDebugAPI {
    const api = createDebugAPI();
    if (worker) {
        state.worker = worker;
    }
    (globalThis as unknown as { __wasmDebug: WasmDebugAPI }).__wasmDebug = api;

    console.log(
        '%c[WasmDebug] Debug API installed. Try: __wasmDebug.status()',
        'color: #4fc3f7; font-weight: bold',
    );
    console.log(
        '%cAvailable commands:\n' +
        '  __wasmDebug.status()            - Overview of WASM state\n' +
        '  __wasmDebug.pendingImports()    - List stuck imports\n' +
        '  __wasmDebug.traceImports(true)  - Enable verbose tracing\n' +
        '  __wasmDebug.probeWorker()       - Probe Worker for state\n' +
        '  __wasmDebug.importHistory(20)   - Recent import calls\n' +
        '  __wasmDebug.hangDetector(5000)  - Configure hang threshold\n' +
        '  __wasmDebug.workerTrace(true)   - Enable tracing in Worker\n' +
        '  __wasmDebug.wrapShims()         - Wrap main-thread shims',
        'color: #888',
    );

    return api;
}

/**
 * Set the Worker reference after the bridge is started.
 */
export function setDebugWorker(worker: Worker): void {
    state.worker = worker;
}
