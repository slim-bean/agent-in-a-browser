/**
 * TUI Loader - Connects ghostty-web terminal to codex-wasm-tui WASM
 *
 * This module provides the bridge between ghostty-web's terminal emulator
 * and the Codex CLI TUI running as a WASM component.
 */

// Import ghostty-web terminal
import { init as initGhostty, Terminal } from 'ghostty-web';

// Import the Codex TUI WASM module (transpiled with jco)
import { run } from '../codex-tui/codex-wasm-tui.js';

// Import the CLI shim to set up the terminal and environment
import { setTerminal, setTerminalSize, setEnvironment } from '@tjfontaine/wasi-shims/ghostty-cli-shim.js';

// Import transport handler for routing MCP requests
import { setTransportHandler, setNetworkApprovalHandler } from '@tjfontaine/wasi-shims/wasi-http-impl.js';

// Import shell exec handler registration for Codex TUI command execution
import { setExecHandler, setApprovalHandler, type ExecEnv, type ExecResult } from '@tjfontaine/wasi-shims/shell-exec-impl.js';

// Import PTY handler registration for persistent shell sessions
import {
    setPtyHandler,
    type PtyHandler,
    type PtyStartParams,
    type PtyStartResult,
    type PtyReadResult,
    type PtyWriteResult,
} from '@tjfontaine/wasi-shims/shell-pty-impl.js';

// Import sandbox for MCP routing
import { fetchFromSandbox, initializeSandbox } from '../../agent/sandbox.js';

// Import OPFS filesystem init for shell access
import { initFilesystem } from '@tjfontaine/wasi-shims/opfs-filesystem-impl.js';

export interface TuiLoaderOptions {
    container: HTMLElement;
    fontSize?: number;
    theme?: {
        background?: string;
        foreground?: string;
        cursor?: string;
    };
}

/**
 * Create a transport handler that routes MCP requests through the sandbox worker
 */
function createSandboxTransport() {
    return async (
        method: string,
        url: string,
        headers: Record<string, string>,
        body: Uint8Array | null
    ): Promise<{ status: number; headers: [string, Uint8Array][]; body: Uint8Array }> => {
        // Extract path from URL (e.g., /mcp/message from http://localhost:3000/mcp/message)
        const urlObj = new URL(url);
        const path = urlObj.pathname;

        console.log('[Transport] Routing to sandbox:', method, path);

        // Build fetch options
        const fetchOptions: RequestInit = {
            method,
            headers: headers,
        };

        if (body) {
            fetchOptions.body = new Blob([body as BlobPart]);
        }

        // Route through sandbox worker
        const response = await fetchFromSandbox(path, fetchOptions);

        // Convert response
        const responseBody = new Uint8Array(await response.arrayBuffer());
        const responseHeaders: [string, Uint8Array][] = [];
        response.headers.forEach((value, name) => {
            responseHeaders.push([name.toLowerCase(), new TextEncoder().encode(value)]);
        });

        return {
            status: response.status,
            headers: responseHeaders,
            body: responseBody
        };
    };
}

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
        /** Wake counter — increments when new output is available */
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
        const json = await response.json();
        if (json.error) {
            throw new Error(json.error.message || `${name} failed`);
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

        // Initialize local session state
        this.sessions.set(params.processId, {
            chunks: [],
            nextSeq: 0n,
            exited: false,
            exitCode: undefined,
            wakeSeq: 0n,
            wakeResolvers: [],
        });

        // If argv has a command, execute it as the initial command
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
            }) as { content?: { type: string; text: string }[] };

            // Extract output text from MCP result
            let output = '';
            if (result?.content) {
                output = result.content
                    .filter((c) => c.type === 'text')
                    .map((c) => c.text)
                    .join('\n');
            }

            // Parse exit code from the [exit_code: N] trailer
            const exitMatch = output.match(/\[exit_code:\s*(-?\d+)\]\s*$/);
            if (exitMatch) {
                session.exitCode = parseInt(exitMatch[1], 10);
                // Strip the trailer from visible output
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

            // Wake any pollWake callers
            session.wakeSeq++;
            for (const resolve of session.wakeResolvers.splice(0)) {
                resolve();
            }
        } catch (err) {
            const errMsg = err instanceof Error ? err.message : String(err);

            // Error responses from pty_exec also contain [exit_code: N]
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

        // Helper to collect and prune chunks
        const collectChunks = () => {
            const newChunks = session.chunks.filter(c => c.seq >= minSeq);
            // Prune delivered chunks — keep only unread ones
            if (newChunks.length > 0) {
                const maxDelivered = newChunks[newChunks.length - 1].seq;
                session.chunks = session.chunks.filter(c => c.seq > maxDelivered);
            }
            return newChunks;
        };

        let chunks = collectChunks();

        // If no chunks and caller wants to wait, block briefly for new output
        if (chunks.length === 0 && waitMs && waitMs > 0n) {
            const waitTime = Math.min(Number(waitMs), 500);
            await new Promise<void>(resolve => {
                const timer = setTimeout(resolve, waitTime);
                // Also wake if new output arrives before timeout
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

        // Interpret the written data as a command string
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
            // Wake any blocked pollWake callers
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

        // Wait for new output or timeout
        const currentWake = session.wakeSeq;
        await new Promise<void>(resolve => {
            const timer = setTimeout(resolve, 200);
            session.wakeResolvers.push(() => { clearTimeout(timer); resolve(); });
        });
        return session.wakeSeq;
    }
}

/**
 * Launch the TUI in a container element
 */
export async function launchTui(options: TuiLoaderOptions): Promise<{
    terminal: Terminal;
    stop: () => void;
}> {
    // Initialize the sandbox worker first (for MCP)
    console.log('[TUI Loader] Initializing sandbox...');
    await initializeSandbox();
    console.log('[TUI Loader] Sandbox ready');

    // Set up transport handler to route MCP requests through sandbox
    setTransportHandler(createSandboxTransport());
    console.log('[TUI Loader] Transport handler configured');

    // Register shell exec handler — routes Codex TUI command execution
    // through the sandbox worker's MCP shell tool
    setExecHandler(async (
        program: string,
        args: string[],
        env: ExecEnv,
        stdin: Uint8Array | undefined,
        timeoutMs: number | undefined,
    ): Promise<ExecResult> => {
        const command = [program, ...args].join(' ');
        console.log('[TUI Loader] Shell exec:', command, 'cwd:', env.cwd);
        const encoder = new TextEncoder();

        try {
            // Route through MCP shell tool via sandbox worker
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

            const result = await response.json();

            if (result.error) {
                return {
                    exitCode: 1,
                    stdout: new Uint8Array(0),
                    stderr: encoder.encode(result.error.message || 'MCP error'),
                };
            }

            // MCP tool result: content is an array of { type, text } items
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
            console.error('[TUI Loader] Shell exec error:', err);
            return {
                exitCode: 127,
                stdout: new Uint8Array(0),
                stderr: encoder.encode(`exec failed: ${err instanceof Error ? err.message : String(err)}`),
            };
        }
    });
    console.log('[TUI Loader] Shell exec handler registered');

    // Register PTY session handler for persistent shell sessions
    setPtyHandler(new PtySessionManager());
    console.log('[TUI Loader] PTY session handler registered');

    // Register policy approval handlers — TUI mode auto-allows everything
    // to maintain backward compatibility with the existing behavior where
    // the TUI has its own in-WASM approval overlay (dangerously_bypass_approvals_and_sandbox).
    setApprovalHandler(async () => 'allow');
    setNetworkApprovalHandler(async () => 'allow');
    console.log('[TUI Loader] Policy approval handlers registered (auto-allow)');

    // Initialize OPFS filesystem for shell access (touch, mkdir, ls, etc.)
    console.log('[TUI Loader] Initializing OPFS filesystem...');
    await initFilesystem();
    console.log('[TUI Loader] OPFS filesystem ready');

    // Initialize ghostty-web
    await initGhostty();

    // Create terminal with sensible defaults
    const terminal = new Terminal({
        fontSize: options.fontSize ?? 14,
        theme: {
            background: options.theme?.background ?? '#1a1b26',
            foreground: options.theme?.foreground ?? '#a9b1d6',
            cursor: options.theme?.cursor ?? '#c0caf5',
        },
    });

    // Mount terminal
    terminal.open(options.container);

    // Load FitAddon for proper sizing - cast because types may be incomplete
    const fitAddon = new (await import('ghostty-web')).FitAddon();
    terminal.loadAddon(fitAddon);
    fitAddon.fit();

    // Explicitly resize to ensure ghostty's internal buffer is allocated
    terminal.resize(terminal.cols, terminal.rows);

    // Small delay to let ghostty finish internal buffer setup
    await new Promise(resolve => setTimeout(resolve, 50));

    fitAddon.observeResize();
    console.log('[TUI Loader] Terminal fitted:', terminal.cols, 'x', terminal.rows);

    // Add keyboard handler for copy/paste and browser shortcuts
    terminal.attachCustomKeyEventHandler((event: KeyboardEvent) => {
        const isMac = navigator.platform.includes('Mac');
        const modKey = isMac ? event.metaKey : event.ctrlKey;

        // On Mac, Cmd+key triggers browser actions; Ctrl+key goes to terminal
        // On non-Mac, Ctrl+key with Shift triggers browser actions; plain Ctrl+key goes to terminal
        const isBrowserShortcut = isMac ? event.metaKey : (event.ctrlKey && event.shiftKey);

        // Handle copy: Mod+C with selection
        if (modKey && event.key === 'c' && terminal.hasSelection()) {
            const selection = terminal.getSelection();
            if (selection) {
                navigator.clipboard.writeText(selection).then(() => {
                    console.log('[TUI] Copied to clipboard:', selection.length, 'chars');
                }).catch(err => {
                    console.error('[TUI] Failed to copy:', err);
                });
                terminal.clearSelection();
            }
            return true; // Prevent default (don't send Ctrl+C to terminal when copying)
        }

        // Handle paste: Mod+V — read clipboard and call terminal.paste()
        // which wraps with bracketed paste markers (ESC[200~ ... ESC[201~).
        // We preventDefault to stop the browser's native paste event from also
        // firing on ghostty-web's hidden textarea (which would double-paste).
        // Return false to prevent ghostty-web from sending raw Ctrl+V (0x16).
        if (modKey && event.key === 'v') {
            event.preventDefault();
            navigator.clipboard.readText().then(text => {
                if (text) {
                    terminal.paste(text);
                }
            }).catch(() => {});
            return false;
        }

        // Handle select all: Cmd+A (Mac) or Ctrl+Shift+A (non-Mac)
        // Plain Ctrl+A on non-Mac goes to terminal as readline beginning-of-line
        if (isBrowserShortcut && event.key === 'a') {
            terminal.selectAll();
            return true; // Prevent default
        }

        // Let browser handle refresh (Cmd+R / Ctrl+R) and dev tools (Cmd+Opt+I / Ctrl+Shift+I)
        if (modKey && (event.key === 'r' || event.key === 'i' || event.key === 't')) {
            return false; // Let browser handle these
        }

        // Let readline shortcuts (Ctrl+A/E/W/K/U) pass through to terminal
        // They'll be sent as control characters via onData
        return false;
    });

    // Wire terminal to our CLI shims
    setTerminal(terminal);

    // Set initial size BEFORE starting TUI so ghostty buffer is properly sized
    console.log('[TUI Loader] Setting initial size before run:', terminal.cols, 'x', terminal.rows);
    setTerminalSize(terminal.cols, terminal.rows);

    // Listen for resize events
    terminal.onResize(({ cols, rows }: { cols: number; rows: number }) => {
        console.log('[TUI Loader] Terminal resized:', cols, 'x', rows);
        setTerminalSize(cols, rows);
    });

    let _running = true;
    const stop = () => {
        _running = false;
        setTransportHandler(null); // Clean up transport handler
    };

    // Set environment variables for the Codex TUI.
    // HOME=/ so find_codex_home() resolves to /.codex at the OPFS root.
    setEnvironment([
        ['HOME', '/'],
        ['CODEX_HOME', '/.codex'],
        ['TERM', 'xterm-256color'],
        ['RUST_BACKTRACE', '1'],
        // Sentinel URL so EnvironmentManager takes the remote exec path,
        // routing unified_exec through our WIT shell-pty backend.
        ['CODEX_EXEC_SERVER_URL', 'wasm-host'],
        // OAuth redirect URI origin — must match the page URL so the
        // callback lands on our domain (localhost in dev, production domain in prod).
        ['CODEX_ORIGIN', globalThis.location?.origin ?? 'https://agent.edge-agent.dev'],
    ]);

    // Pre-create /.codex in OPFS so find_codex_home() succeeds.
    try {
        const root = await navigator.storage.getDirectory();
        await root.getDirectoryHandle('.codex', { create: true });
        console.log('[TUI Loader] Pre-created /.codex in OPFS');
    } catch (e) {
        console.warn('[TUI Loader] Failed to pre-create .codex in OPFS:', e);
    }

    // Show loading indicator while WASM initializes
    terminal.write('\r\n  Loading Codex...\r\n');

    // Run the TUI (async via JSPI).
    // The TUI startup does ~2s of synchronous work (config loading, app init)
    // before reaching its first JSPI suspend point (blocking_read on stdin).
    // Use requestAnimationFrame to ensure the loading message renders first.
    await new Promise<void>(resolve => {
        requestAnimationFrame(() => {
            console.log('[TUI Loader] Calling run()...');

            let tuiDone = false;
            run().then(exitCode => {
                tuiDone = true;
                console.log('TUI exited with code:', exitCode);
            }).catch(err => {
                tuiDone = true;
                console.error('TUI error:', err);
            });

            // Deadlock watchdog: active monitoring of WASM progress.
            // Uses both stderr output tracking AND a direct activity flag
            // that the stdin read timeout resets (every 33ms when healthy).
            let lastActivityTime = Date.now();
            const WATCHDOG_INTERVAL_MS = 5000;  // check every 5s
            const STALL_THRESHOLD_MS = 30000;   // 30s without activity = stalled
            let stallWarned = false;

            // Activity hooks — track three signals:
            // 1. stderr writes (WASM producing output)
            // 2. stdin reads (WASM reading user input)
            // 3. yield activity (cooperative scheduler yielding to JS via JSPI)
            (globalThis as any).__wasmStderrTime = () => {
                lastActivityTime = Date.now();
                stallWarned = false;
            };
            (globalThis as any).__wasmStdinActivity = () => {
                lastActivityTime = Date.now();
                stallWarned = false;
            };
            (globalThis as any).__wasmYieldActivity = () => {
                lastActivityTime = Date.now();
                stallWarned = false;
            };

            const watchdog = setInterval(() => {
                if (tuiDone) {
                    clearInterval(watchdog);
                    return;
                }
                const stalled = Date.now() - lastActivityTime;
                if (stalled > STALL_THRESHOLD_MS && !stallWarned) {
                    stallWarned = true;
                    console.error(
                        `[TUI Watchdog] WASM stalled — no activity for ${Math.round(stalled / 1000)}s. ` +
                        `The cooperative scheduler is not yielding. This means a task ` +
                        `is blocking inside poll_spawned_tasks or the main future. ` +
                        `Check the last [poll_tasks] and [block_on] messages above ` +
                        `to identify the blocking task.`
                    );
                } else if (stalled > STALL_THRESHOLD_MS) {
                    // Keep reporting every interval
                    console.warn(
                        `[TUI Watchdog] Still stalled (${Math.round(stalled / 1000)}s)`
                    );
                }
            }, WATCHDOG_INTERVAL_MS);

            // run() returns a Promise immediately (JSPI), resolve to continue
            resolve();
        });
    });

    return { terminal, stop };
}

/**
 * Export for simple usage
 */
export { Terminal };
