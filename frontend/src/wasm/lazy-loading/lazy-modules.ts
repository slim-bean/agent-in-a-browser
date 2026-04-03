/**
 * Lazy Module Loader
 * 
 * Dynamically loads heavy WASM modules (tsx-engine, sqlite-module) on demand.
 * This reduces initial load time by deferring these modules until first use.
 * 
 * Supports dual async modes:
 * - JSPI mode (Chrome): True lazy loading with async suspension
 * - Sync mode (Safari/Firefox): Eager loading at startup
 */

import { hasJSPI } from './async-mode.js';

// Import and re-export from wasm-loader for unified API
import {
    registerModule,
    getModuleRegistration,
    isRegisteredCommand,
    isInteractiveCommand as isInteractiveCommandRegistry,
    getModuleForCommand as getModuleForCommandRegistry,
    getAllCommands,
    setTerminalContext,
    isTerminalContext,
    type CommandModule,
    type InputStream,
    type OutputStream,
    type ExecEnv,
    type CommandHandle,
} from '@tjfontaine/wasm-loader';

// Import metadata from wasm-* packages (no dynamic imports, safe for Rollup)
// Loaders are attached locally when registering to avoid build-time resolution
import { metadata as tsxMetadata } from '@tjfontaine/wasm-tsx';
import { metadata as sqliteMetadata } from '@tjfontaine/wasm-sqlite';
import { metadata as vimMetadata } from '@tjfontaine/wasm-vim';
import { metadata as stripeMetadata } from '@tjfontaine/wasm-stripe';
import { metadata as gitMetadata } from '@tjfontaine/wasm-git';
import { metadata as pythonMetadata } from '@tjfontaine/wasm-python';

// Import types for internal use (these modules are still loaded by our loaders for now)
type TsxEngineModule = typeof import('@tjfontaine/wasm-tsx/wasm/tsx-engine.js');
type SqliteModule = typeof import('@tjfontaine/wasm-sqlite/wasm/sqlite-module.js');
// Note: StripeModule type will resolve after `moon run wasm-stripe:transpile`
// type _StripeModule = typeof import('@tjfontaine/wasm-stripe/wasm/stripe-module.js');

// Re-export types from wasm-loader for consumers
export type { CommandModule, CommandHandle, InputStream, OutputStream, ExecEnv };

// Cache for loaded modules
const loadedModules: Map<string, CommandModule> = new Map();

// Loading promises to prevent double-loading
const loadingPromises: Map<string, Promise<CommandModule>> = new Map();

// Re-export terminal context functions and utilities from wasm-loader
export { setTerminalContext, isTerminalContext, getAllCommands };

// ============================================================================
// Module Registration - Initialize at startup
// ============================================================================

let _modulesRegistered = false;

/**
 * Register all WASM modules.
 * Call this at startup before using any lazy-loaded commands.
 * 
 * Packages export only metadata (name, commands) - no loader functions.
 * Loaders are attached here to avoid Rollup resolving dynamic imports at build time.
 */
export function registerAllModules(): void {
    if (_modulesRegistered) return;

    console.log('[LazyLoader] Registering WASM modules...');

    // Combine package metadata with local loader functions
    registerModule({ ...tsxMetadata, loader: loadTsxEngine });
    registerModule({ ...sqliteMetadata, loader: loadSqliteModule });
    registerModule({ ...vimMetadata, loader: loadEdtuiModule });
    registerModule({ ...stripeMetadata, loader: loadStripeModule });
    registerModule({ ...gitMetadata, loader: loadGitModule });
    registerModule({ ...pythonMetadata, loader: loadPyodideModule });

    _modulesRegistered = true;
    console.log('[LazyLoader] All modules registered');
}

/**
 * Commands that are handled by lazy-loaded modules
 */
export const LAZY_COMMANDS: Record<string, string> = {
    'tsx': 'tsx-engine',
    'tsc': 'tsx-engine',
    'sqlite3': 'sqlite-module',
    // Vim-style editor
    'vim': 'edtui-module',
    'vi': 'edtui-module',
    'edit': 'edtui-module',
    // Python (Pyodide)
    'python3': 'pyodide-module',
    'python': 'pyodide-module',
    'pip': 'pyodide-module',
    // Stripe CLI
    'stripe': 'stripe-module',
    // Interactive shell (uses main runtime's shell:unix/command export)
    'sh': 'brush-shell',
    'shell': 'brush-shell',
    'bash': 'brush-shell',
    // Codex TUI (AI agent, launched from shell)
    'codex': 'codex-tui',
};

/**
 * Commands that are interactive TUI apps (need direct terminal access).
 * These commands use spawn_interactive and bypass shell output buffering.
 * NOTE: Most of these are now derived from the registry. This set contains
 * any extra commands not yet in packages.
 */
export const INTERACTIVE_COMMANDS = new Set<string>([
    // Any non-packaged interactive commands go here
]);

/**
 * Check if a command is an interactive TUI (needs spawn_interactive).
 * Uses the registry if the command is registered, otherwise falls back to local check.
 */
export function isInteractiveCommand(commandName: string): boolean {
    // First check the registry (for packaged modules)
    if (isRegisteredCommand(commandName)) {
        return isInteractiveCommandRegistry(commandName);
    }
    // Fall back to legacy check
    return INTERACTIVE_COMMANDS.has(commandName);
}

/**
 * Check if a command should be lazy-loaded.
 * Uses the registry if registered, otherwise checks LAZY_COMMANDS.
 */
export function isLazyCommand(commandName: string): boolean {
    return isRegisteredCommand(commandName) || commandName in LAZY_COMMANDS;
}

/**
 * Get the module name for a lazy command.
 * Uses the registry if registered, otherwise checks LAZY_COMMANDS.
 */
export function getModuleForCommand(commandName: string): string | undefined {
    const registryModule = getModuleForCommandRegistry(commandName);
    if (registryModule) {
        return registryModule;
    }
    return LAZY_COMMANDS[commandName];
}


/**
 * Wrap a sync module (with run()) to provide spawn() interface
 */
function wrapSyncModule(syncModule: { run: TsxEngineModule['command']['run']; listCommands: TsxEngineModule['command']['listCommands'] }): CommandModule {
    return {
        spawn(name, args, env, stdin, stdout, stderr) {
            console.log(`[wrapSyncModule] spawn() called: name=${name}, args=`, args);
            console.log(`[wrapSyncModule] Calling syncModule.run()...`);
            // Execute synchronously and return immediately-resolved handle
            const exitCode = syncModule.run(name, args, env, stdin, stdout, stderr);
            console.log(`[wrapSyncModule] syncModule.run() returned exitCode=${exitCode}`);
            return {
                poll: () => {
                    console.log(`[wrapSyncModule] poll() called, returning ${exitCode}`);
                    return exitCode;
                },
                resolve: () => {
                    console.log(`[wrapSyncModule] resolve() called, resolving with ${exitCode}`);
                    return Promise.resolve(exitCode);
                },
            };
        },
        listCommands: () => syncModule.listCommands(),
    };
}

/**
 * Load the tsx-engine module
 */
async function loadTsxEngine(): Promise<CommandModule> {
    console.log('[LazyLoader] Loading tsx-engine module...');
    const startTime = performance.now();

    // Dynamic import based on JSPI support
    // Safari needs sync variant to avoid WebAssembly.Suspending error
    let module: TsxEngineModule;
    if (hasJSPI) {
        module = await import('@tjfontaine/wasm-tsx/wasm/tsx-engine.js');
    } else {
        module = await import('@tjfontaine/wasm-tsx/wasm-sync/tsx-engine.js');
    }

    // With --tla-compat, we must await $init before accessing exports
    if ('$init' in module) {
        await (module as { $init: Promise<void> }).$init;
    }

    const loadTime = performance.now() - startTime;
    console.log(`[LazyLoader] tsx-engine loaded in ${loadTime.toFixed(0)}ms`);

    // Use JSPI wrapper when available since poll-impl.js makes run() async
    if (hasJSPI) {
        return wrapJspiModule(module.command as unknown as Parameters<typeof wrapJspiModule>[0]);
    }
    // Wrap the sync command interface to provide spawn()
    return wrapSyncModule(module.command);
}

/**
 * Load the sqlite-module
 */
async function loadSqliteModule(): Promise<CommandModule> {
    console.log('[LazyLoader] Loading sqlite-module...');
    const startTime = performance.now();

    // Dynamic import based on JSPI support
    // Safari needs sync variant to avoid WebAssembly.Suspending error
    let module: SqliteModule;
    if (hasJSPI) {
        module = await import('@tjfontaine/wasm-sqlite/wasm/sqlite-module.js');
    } else {
        module = await import('@tjfontaine/wasm-sqlite/wasm-sync/sqlite-module.js');
    }

    // With --tla-compat, we must await $init before accessing exports
    if ('$init' in module) {
        await (module as { $init: Promise<void> }).$init;
    }

    const loadTime = performance.now() - startTime;
    console.log(`[LazyLoader] sqlite-module loaded in ${loadTime.toFixed(0)}ms`);

    // Use JSPI wrapper when available since poll-impl.js makes run() async
    if (hasJSPI) {
        return wrapJspiModule(module.command as unknown as Parameters<typeof wrapJspiModule>[0]);
    }
    // Wrap the sync command interface to provide spawn()
    // Cast through unknown: sqlite's JCO-generated InputStream includes subscribe()
    // (used by libsqlite3-sys internally) while tsx's does not, causing structural mismatch.
    return wrapSyncModule(module.command as unknown as Parameters<typeof wrapSyncModule>[0]);
}

/**
 * Load the git-module (Go CLI via go-git)
 *
 * Go-compiled git CLI using go-git, adapted from wasip1 to wasip2 component model.
 * Uses the same direct wasip1 loader pattern as stripe-module.
 */
async function loadGitModule(): Promise<CommandModule> {
    console.log('[LazyLoader] Loading git-module (Go CLI, direct wasip1)...');
    const startTime = performance.now();

    const { loadGoWasip1Module, GoWasmExit } = await import('./go-wasip1-loader.js');
    const httpBridge = await import('@tjfontaine/wasi-shims/http-bridge-impl.js');

    const wasmUrl = '/wasm-git/git.wasm';

    const loadTime = performance.now() - startTime;
    console.log(`[LazyLoader] git-module imports loaded in ${loadTime.toFixed(0)}ms`);

    return createDirectGoAdapter(loadGoWasip1Module, GoWasmExit, wasmUrl, httpBridge);
}

/**
 * Wrap a JSPI-transpiled module (with async run()) to provide spawn() interface
 * 
 * Unlike wrapSyncModule, the run() function returns a Promise that resolves
 * when the command completes. JSPI allows the WASM stack to suspend on
 * blocking-read calls, returning control to JavaScript.
 */
function wrapJspiModule(jspiModule: {
    run: (name: string, args: string[], env: ExecEnv, stdin: InputStream, stdout: OutputStream, stderr: OutputStream) => Promise<number>;
    listCommands: TsxEngineModule['command']['listCommands']
}): CommandModule {
    return {
        spawn(name, args, env, stdin, stdout, stderr) {
            console.log(`[wrapJspiModule] spawn() called: name=${name}, args=`, args);

            // Start the async execution
            let exitCode: number | undefined = undefined;
            let resolvePromise: ((code: number) => void) | null = null;
            let rejectPromise: ((err: Error) => void) | null = null;

            // Create the execution promise
            const executionPromise = new Promise<number>((resolve, reject) => {
                resolvePromise = resolve;
                rejectPromise = reject;
            });

            // Start the JSPI run - this will suspend on blocking-read
            console.log(`[wrapJspiModule] Calling jspiModule.run() (async)...`);
            jspiModule.run(name, args, env, stdin, stdout, stderr)
                .then(code => {
                    console.log(`[wrapJspiModule] jspiModule.run() resolved with exitCode=${code}`);
                    exitCode = code;
                    resolvePromise?.(code);
                })
                .catch(err => {
                    console.error(`[wrapJspiModule] jspiModule.run() rejected:`, err);
                    exitCode = 1;
                    rejectPromise?.(err);
                });

            return {
                poll: () => exitCode,
                resolve: () => executionPromise,
            };
        },
        listCommands: () => jspiModule.listCommands(),
    };
}

/**
 * Load the ratatui-demo module (interactive TUI demo)
 * 
 * This module is transpiled with JSPI mode and async stdin imports/exports.
 * When the TUI calls stdin.blockingRead(), JSPI suspends the WASM stack
 * and returns control to JavaScript, allowing the event loop to deliver
 * keyboard input.
 */
/**
 * Load the edtui-module (vim-style editor)
 * 
 * Supports both JSPI and sync modes for interactive editing.
 * In sync mode, uses the WorkerBridge stdin mechanism for keyboard input.
 */
async function loadEdtuiModule(): Promise<CommandModule> {
    console.log('[LazyLoader] Loading edtui-module (vim editor)...');
    const startTime = performance.now();

    // Dynamic import based on JSPI support
    // Safari needs sync variant to avoid WebAssembly.Suspending error
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    let module: any;
    if (hasJSPI) {
        module = await import('@tjfontaine/wasm-vim/wasm/edtui-module.js');
    } else {
        module = await import('@tjfontaine/wasm-vim/wasm-sync/edtui-module.js');
    }

    // Await $init for the module initialization
    if (module.$init) {
        await module.$init;
    }

    const loadTime = performance.now() - startTime;
    console.log(`[LazyLoader] edtui-module loaded in ${loadTime.toFixed(0)}ms`);

    // Use JSPI wrapper when available since poll-impl.js makes run() async
    if (hasJSPI) {
        return wrapJspiModule(module.command as unknown as Parameters<typeof wrapJspiModule>[0]);
    }
    // Wrap the sync command interface to provide spawn()
    return wrapSyncModule(module.command);
}

/**
 * Load the stripe-module (Stripe CLI)
 *
 * Go-compiled Stripe CLI, adapted from wasip1 to wasip2 component model.
 * Unlike Rust modules that export shell:unix/command, the Go component exports
 * wasi:cli/run (standard CLI entry point). We wrap it with a JS adapter that
 * configures the WASI CLI shims (args, env, streams) before calling run().
 */
async function loadStripeModule(): Promise<CommandModule> {
    console.log('[LazyLoader] Loading stripe-module (Go CLI, direct wasip1)...');
    const startTime = performance.now();

    // Import the direct wasip1 loader — bypasses the WASM Component Model
    // to avoid stack overflow from adapter + JCO trampoline overhead.
    // The raw wasip1 Go binary works fine; the component model adds too many
    // call stack frames for Go's 568 init functions.
    const { loadGoWasip1Module, GoWasmExit } = await import('./go-wasip1-loader.js');

    // Import bridges: HTTP for API calls, WebSocket for `stripe listen`,
    // and browser actions for `stripe login` URL opening
    const httpBridge = await import('@tjfontaine/wasi-shims/http-bridge-impl.js');
    const wsBridge = await import('@tjfontaine/wasi-shims/ws-bridge-impl.js');
    const { openUrl } = await import('@tjfontaine/wasi-shims/browser-impl.js');

    // The raw wasip1 binary is served from /wasm-stripe/stripe.wasm
    const wasmUrl = '/wasm-stripe/stripe.wasm';

    const loadTime = performance.now() - startTime;
    console.log(`[LazyLoader] stripe-module imports loaded in ${loadTime.toFixed(0)}ms`);

    return createDirectGoAdapter(loadGoWasip1Module, GoWasmExit, wasmUrl, httpBridge, wsBridge, openUrl);
}

/**
 * Create a direct Go CLI adapter that instantiates the raw wasip1 binary
 * per invocation, bypassing the WASM Component Model entirely.
 *
 * Go's _start() can only run once per WASM instance, so we create a fresh
 * instance for each command invocation. The raw binary is ~37MB but compilation
 * is cached by the browser after the first load.
 */
function createDirectGoAdapter(
    loadGoWasip1Module: typeof import('./go-wasip1-loader.js')['loadGoWasip1Module'],
    _GoWasmExit: typeof import('./go-wasip1-loader.js')['GoWasmExit'],
    wasmUrl: string,
    httpBridge: {
        request: (method: string, url: string, headers: string, body: Uint8Array) => number | Promise<number>;
        responseStatus: (handle: number) => number;
        responseHeaders: (handle: number) => string;
        responseBodyRead: (handle: number, maxBytes: number) => Uint8Array;
        responseClose: (handle: number) => void;
    },
    wsBridge?: {
        connect: (url: string) => number | Promise<number>;
        read: (handle: number, maxBytes: number) => Uint8Array | Promise<Uint8Array>;
        write: (handle: number, data: Uint8Array) => number;
        close: (handle: number) => void;
    },
    openUrl?: (url: string) => void | Promise<void>,
): CommandModule {
    // Extract exit code using duck typing instead of instanceof.
    // GoWasmExit has { exitError: true, code: number }. Using instanceof
    // fails when Vite code-splits go-wasip1-loader into a separate chunk,
    // creating a different class identity than the one thrown by proc_exit.
    const extractExitCode = (err: unknown): number => {
        if (err && typeof err === 'object' && 'exitError' in err) {
            return (err as { code?: number }).code ?? 1;
        }
        return 1;
    };

    return {
        spawn(name, args, env, _stdin, stdout, stderr) {
            console.log(`[GoDirectAdapter] spawn: name=${name}, args=`, args);

            let exitCode: number | undefined;

            const executionPromise = (async () => {
                try {
                    const goInstance = await loadGoWasip1Module(wasmUrl, {
                        args: [name, ...args],
                        env: env.vars,
                        cwd: env.cwd,
                        stdoutWrite: (data) => stdout.write(data),
                        stderrWrite: (data) => stderr.write(data),
                        httpBridge,
                        wsBridge,
                        openUrl,
                    });

                    await goInstance.run();
                    exitCode = 0;
                    return 0;
                } catch (err: unknown) {
                    exitCode = extractExitCode(err);
                    if (exitCode > 1) {
                        // Only log unexpected errors, not normal proc_exit
                        console.error('[GoDirectAdapter] run() error:', err);
                    }
                    return exitCode;
                }
            })();

            return {
                poll: () => exitCode,
                resolve: () => executionPromise,
            };
        },
        listCommands: () => [wasmUrl.includes('stripe') ? 'stripe' : 'git'],
    };
}

/**
 * Load the pyodide-module (Python runtime)
 *
 * Pyodide is a CPython port to WebAssembly. Unlike Rust/Go WASM modules,
 * it's a pre-built Emscripten binary with its own JS glue and loading mechanism.
 * We wrap it with a JS adapter that implements the CommandModule interface.
 *
 * The Pyodide instance is cached as a singleton — Python startup is expensive
 * so we keep the interpreter alive between invocations.
 *
 * OPFS is mounted into Pyodide's virtual FS via mountNativeFS() so Python
 * has seamless access to the same files the shell uses.
 */
async function loadPyodideModule(): Promise<CommandModule> {
    console.log('[LazyLoader] Loading pyodide-module (Python runtime)...');
    const startTime = performance.now();

    const { createPyodideModule } = await import('./pyodide-loader.js');

    const loadTime = performance.now() - startTime;
    console.log(`[LazyLoader] pyodide-module loader imported in ${loadTime.toFixed(0)}ms`);

    return createPyodideModule();
}

/**
 * Load the brush-shell from the main MCP server (interactive shell)
 * 
 * The main runtime now exports shell:unix/command alongside wasi:http/incoming-handler.
 * This gives the interactive shell access to all 50+ shell commands.
 * Supports both JSPI and sync modes for interactive shell access.
 */
async function loadBrushShell(): Promise<CommandModule> {
    console.log('[LazyLoader] Loading interactive shell from MCP server...');
    const startTime = performance.now();

    // Dynamic import based on JSPI support
    // Safari needs sync variant to avoid WebAssembly.Suspending error
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    let module: any;
    if (hasJSPI) {
        module = await import('@tjfontaine/mcp-wasm-server/mcp-server-jspi/ts-runtime-mcp.js');
    } else {
        module = await import('@tjfontaine/mcp-wasm-server/mcp-server-sync/ts-runtime-mcp.js');
    }

    // Await $init for the module initialization
    if (module.$init) {
        await module.$init;
    }

    const loadTime = performance.now() - startTime;
    console.log(`[LazyLoader] Interactive shell loaded in ${loadTime.toFixed(0)}ms`);

    // Use JSPI wrapper when available since poll-impl.js makes run() async
    if (hasJSPI) {
        return wrapJspiModule(module.command as unknown as Parameters<typeof wrapJspiModule>[0]);
    }
    // Wrap the sync command interface to provide spawn()
    return wrapSyncModule(module.command);
}

/**
 * Load the Codex TUI as a lazy-loaded interactive command.
 *
 * The Codex TUI module exports `run() -> s32` (reads stdin/stdout from wasi:cli).
 * We wrap it to match the CommandModule interface. The ghostty-cli-shim already
 * provides stdin/stdout/stderr, so the TUI will inherit the terminal streams.
 */
async function loadCodexTui(): Promise<CommandModule> {
    console.log('[LazyLoader] Loading Codex TUI...');
    const startTime = performance.now();

    const tuiModule = await import('../codex-tui/codex-wasm-tui.js');

    const loadTime = performance.now() - startTime;
    console.log(`[LazyLoader] Codex TUI loaded in ${loadTime.toFixed(0)}ms`);

    // Expose pushAuthCallback so the worker message handler can route OAuth
    // callbacks into the tiny_http channel inside the WASM module.
    if (typeof tuiModule.pushAuthCallback === 'function') {
        (globalThis as Record<string, unknown>).__pushAuthCallback = tuiModule.pushAuthCallback;
        console.log('[LazyLoader] pushAuthCallback registered on globalThis');
    }

    // Import the CLI shim to set arguments before TUI runs
    const cliShim = await import('@tjfontaine/wasi-shims/ghostty-cli-shim.js');

    return {
        spawn(name, args, _env, _stdin, _stdout, _stderr) {
            // Set CLI arguments so the TUI's clap parser can read them
            // via wasi:cli/environment::get_arguments()
            cliShim.setArguments([name, ...args]);

            let exitCode: number | undefined = undefined;
            let resolvePromise: ((code: number) => void) | null = null;
            let rejectPromise: ((err: Error) => void) | null = null;

            const executionPromise = new Promise<number>((resolve, reject) => {
                resolvePromise = resolve;
                rejectPromise = reject;
            });

            // The TUI's run() reads from wasi:cli/stdin (ghostty-cli-shim)
            tuiModule.run()
                .then((code: number) => {
                    console.log(`[LazyLoader] Codex TUI exited with code: ${code}`);
                    exitCode = code;
                    resolvePromise?.(code);
                })
                .catch((err: Error) => {
                    console.error(`[LazyLoader] Codex TUI error:`, err);
                    exitCode = 1;
                    rejectPromise?.(err);
                });

            return {
                poll: () => exitCode,
                resolve: () => executionPromise,
            };
        },
        listCommands: () => ['codex'],
    };
}

/**
 * Register the Codex TUI as a lazy-loaded interactive command.
 * Called from the worker after module setup is complete.
 */
export function registerCodexTui(): void {
    registerModule({
        name: 'codex-tui',
        commands: [
            { name: 'codex', mode: 'tui' as const },
        ],
        loader: loadCodexTui,
    });
    console.log('[LazyLoader] Codex TUI registered as interactive command');
}

/**
 * Load a lazy module by name
 */
export async function loadLazyModule(moduleName: string): Promise<CommandModule> {
    // Check if already loaded
    const cached = loadedModules.get(moduleName);
    if (cached) {
        console.log(`[LazyLoader] ${moduleName} already loaded (cached)`);
        return cached;
    }

    // Check if currently loading
    const existingPromise = loadingPromises.get(moduleName);
    if (existingPromise) {
        console.log(`[LazyLoader] ${moduleName} already loading, waiting...`);
        return existingPromise;
    }

    // Look up the module's loader from the registry
    const registration = getModuleRegistration(moduleName);
    if (!registration) {
        throw new Error(`Unknown lazy module: ${moduleName}`);
    }

    const loadPromise = registration.loader();

    loadingPromises.set(moduleName, loadPromise);

    try {
        const module = await loadPromise;
        loadedModules.set(moduleName, module);
        loadingPromises.delete(moduleName);
        return module;
    } catch (error) {
        loadingPromises.delete(moduleName);
        throw error;
    }
}

/**
 * Load the module for a specific command and return it
 */
export async function loadModuleForCommand(commandName: string): Promise<CommandModule | null> {
    const moduleName = LAZY_COMMANDS[commandName];
    if (!moduleName) {
        return null;
    }

    return loadLazyModule(moduleName);
}

/**
 * Get list of all lazy-loadable commands
 */
export function getLazyCommandList(): string[] {
    return Object.keys(LAZY_COMMANDS);
}

/**
 * Check if a module is already loaded
 */
export function isModuleLoaded(moduleName: string): boolean {
    return loadedModules.has(moduleName);
}

/**
 * Get a module synchronously (returns null if not loaded yet)
 */
export function getLoadedModuleSync(moduleName: string): CommandModule | null {
    return loadedModules.get(moduleName) ?? null;
}

/**
 * Trigger async preloading of a module (fire-and-forget)
 * This starts loading in the background so it's ready when needed.
 */
export function preloadModule(moduleName: string): void {
    if (loadedModules.has(moduleName) || loadingPromises.has(moduleName)) {
        return; // Already loaded or loading
    }

    console.log(`[LazyLoader] Preloading ${moduleName} in background...`);
    loadLazyModule(moduleName).catch(err => {
        console.error(`[LazyLoader] Failed to preload ${moduleName}:`, err);
    });
}

/**
 * Initialize all lazy modules eagerly.
 * Used in Sync mode (Safari/Firefox) where we can't do async suspension.
 * In JSPI mode (Chrome), this is not needed as modules load on-demand.
 */
export async function initializeForSyncMode(): Promise<void> {
    if (hasJSPI) {
        console.log('[LazyLoader] JSPI available, skipping eager load');
        return;
    }

    console.log('[LazyLoader] Sync mode - eager loading all lazy modules...');
    const startTime = performance.now();

    // Load all lazy modules in parallel
    // All interactive modules now support sync mode via WorkerBridge stdin mechanism
    const moduleNames = [
        'tsx-engine',
        'sqlite-module',
        'edtui-module',   // Vim editor - now supports sync mode
        // 'git-module' excluded: 17MB Go WASM is too large for eager preload.
        // 'stripe-module' excluded: 35MB Go WASM is too large for eager preload.
        // Both Go CLIs load on-demand even in sync mode.
        'brush-shell',    // Interactive shell - now supports sync mode
    ];
    await Promise.all(moduleNames.map(name =>
        loadLazyModule(name).catch(err => {
            console.error(`[LazyLoader] Failed to eager load ${name}:`, err);
        })
    ));

    const loadTime = performance.now() - startTime;
    console.log(`[LazyLoader] All modules loaded in ${loadTime.toFixed(0)}ms`);
}

// Re-export hasJSPI for consumers
export { hasJSPI };

// Auto-register modules at import time
// This ensures commands like vim are registered before any queries
registerAllModules();
