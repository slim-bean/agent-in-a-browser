/**
 * Main entry point for Web Agent TUI
 *
 * This uses the Rust/ratatui-based TUI instead of the React app.
 * The WASM always runs in a Worker (via WorkerBridge) regardless of
 * JSPI support, because OPFS createSyncAccessHandle requires Worker context.
 *
 * - JSPI browsers (Chrome/Firefox): Worker loads async WASM, uses JSPI suspension
 * - Non-JSPI browsers (Safari): Worker loads sync WASM, uses Atomics.wait()
 */

import './index.css';

// Import OAuth handler to register window.__mcpOAuthHandler
import './oauth-handler.js';

import { hasJSPI } from '@tjfontaine/mcp-wasm-server';
import { WorkerBridge } from '@tjfontaine/wasi-shims';
import { listenForOpenUrl } from '@tjfontaine/wasi-shims/browser-impl.js';

// Debug instrumentation for diagnosing WASM/JSPI hangs
import { installDebugAPI } from './debug/wasm-debug.js';

// Import bundled worker URL - Vite's ?worker&url suffix ensures:
// 1. The worker is bundled as JavaScript (not raw TypeScript)
// 2. We get the correct URL to the bundled worker asset
// This is critical for WebKit which cannot execute TypeScript in workers
import wasmWorkerUrl from './workers/wasm-worker.ts?worker&url';

// Create full-screen terminal container
const root = document.getElementById('root')!;
root.innerHTML = '<div id="terminal" style="width: 100%; height: 100vh;"></div>';

const terminalEl = document.getElementById('terminal')!;

// Listen for open-url requests from WASM workers (opens new tabs)
listenForOpenUrl();

// Auto-launch the TUI
(async () => {
    try {
        console.log('[Main] Launching TUI...');
        console.log(`[Main] JSPI support: ${hasJSPI ? 'YES' : 'NO'}`);

        // Initialize the sandbox worker first (for MCP)
        // This runs ts-runtime-mcp to handle MCP requests
        // Use fetchFromSandboxSimple - MessageChannel ports fail silently in Safari workers
        const { initializeSandbox, fetchFromSandboxSimple, fetchFromSandbox } = await import('./agent/sandbox.js');
        console.log('[Main] Initializing sandbox for MCP...');
        await initializeSandbox();
        console.log('[Main] Sandbox ready');

        // Choose the appropriate sandbox fetch function
        // JSPI path can use the richer fetchFromSandbox; non-JSPI needs fetchFromSandboxSimple
        const sandboxFetchForRelay = hasJSPI ? fetchFromSandbox : fetchFromSandboxSimple;

        // Initialize ghostty-web and create terminal
        const ghostty = await import('ghostty-web');
        await ghostty.init();

        const terminal = new ghostty.Terminal({
            fontSize: 14,
            theme: {
                background: '#1a1b26',
                foreground: '#a9b1d6',
                cursor: '#c0caf5',
            }
        });
        terminal.open(terminalEl);

        // Expose terminal for E2E tests immediately (bridge.runModule doesn't return)
        (window as unknown as { tuiTerminal: unknown }).tuiTerminal = terminal;

        // Register link providers so URLs are clickable and open in new windows
        terminal.registerLinkProvider(new ghostty.UrlRegexProvider(terminal));
        terminal.registerLinkProvider(new ghostty.OSC8LinkProvider(terminal));

        // Load FitAddon for proper sizing
        const fitAddon = new ghostty.FitAddon();
        terminal.loadAddon(fitAddon);
        fitAddon.fit();

        // Create MCP transport handler that routes through sandbox
        const mcpTransport = async (
            method: string,
            url: string,
            headers: Record<string, string>,
            body: Uint8Array | null
        ) => {
            console.log('[Main] mcpTransport called:', method, url);
            // Extract path from URL
            const urlObj = new URL(url);
            const path = urlObj.pathname;

            console.log('[Main] Calling fetchFromSandboxSimple:', path);
            const fetchOptions: RequestInit = { method, headers };
            if (body) fetchOptions.body = new Blob([body as BlobPart]);

            const response = await fetchFromSandboxSimple(path, fetchOptions);
            console.log('[Main] fetchFromSandboxSimple returned:', response.status);
            const responseBody = new Uint8Array(await response.arrayBuffer());

            return { status: response.status, body: responseBody };
        };

        // Launch worker bridge with MCP transport and bundled worker URL
        const workerUrl = new URL(wasmWorkerUrl, import.meta.url);
        const bridge = new WorkerBridge(terminal, { mcpTransport, workerUrl });
        await bridge.start();

        // Install debug API on window.__wasmDebug and connect to Worker
        // The probeWorker() function uses addEventListener which coexists with
        // the bridge's onmessage handler.
        installDebugAPI((bridge as any).getWorker?.() ?? (bridge as any).worker ?? undefined);

        // Wire terminal resize events to WorkerBridge
        terminal.onResize(({ cols, rows }: { cols: number; rows: number }) => {
            console.log('[Main] Terminal resized (ghostty):', cols, 'x', rows);
            bridge.handleResize(cols, rows);
        });

        // Use FitAddon's observeResize for automatic resize handling
        fitAddon.observeResize();
        console.log('[Main] FitAddon initialized:', terminal.cols, 'x', terminal.rows);

        // Send initial size to worker
        bridge.handleResize(terminal.cols, terminal.rows);

        // Run the shell as the default entry point.
        // The `codex` command is available within the shell to launch the Codex TUI.
        // Include terminal dimensions so the CLI shim has the correct size before WASM starts.
        bridge.runModule('shell', undefined, { jspi: hasJSPI, cols: terminal.cols, rows: terminal.rows });

        // Forward OAuth callbacks from the popup to the worker.
        // When the OAuth redirect lands on /oauth-callback, the popup sends
        // a postMessage back to the opener. We forward it to the worker
        // which calls pushAuthCallback on the codex-tui WASM module.
        const worker = bridge.getWorker();
        if (worker) {
            window.addEventListener('message', (event) => {
                if (event.origin !== window.location.origin) return;
                if (event.data?.type === 'oauth-callback' && event.data.code && event.data.state) {
                    console.log('[Main] Forwarding OAuth callback to worker');
                    worker.postMessage({
                        type: 'oauth-callback',
                        code: event.data.code,
                        state: event.data.state,
                    });
                }
            });
        }

        // Focus the terminal
        terminal.focus();

        // ---- Cloud Relay Setup ----
        // If running on a session subdomain, connect the relay so external
        // MCP clients (Claude Code, etc.) can reach the sandbox.
        if (sandboxFetchForRelay) {
            try {
                const { RelayClient, getCurrentSession } = await import('@tjfontaine/edge-agent-session');
                const session = getCurrentSession();

                if (session) {
                    const { mountRelayOverlay } = await import('./relay/RelayStatusOverlay.js');

                    const relay = new RelayClient({
                        sessionId: session.sid,
                        tenantId: session.tenantId,
                        sandboxFetch: sandboxFetchForRelay,
                        onStateChange: (state) => {
                            console.log('[Main] Relay state:', state);
                            overlay.updateState(state);
                        },
                    });

                    const overlay = mountRelayOverlay(relay, session);

                    if (relay.connect()) {
                        // Once connected, tell the relay we're ready
                        relay.sendStatus(true);
                        console.log('[Main] Relay connected for session:', session.sid);
                    }
                } else {
                    console.log('[Main] Not on a session subdomain, relay not started');
                }
            } catch (err) {
                // Relay is non-critical -- don't break the TUI if it fails
                console.warn('[Main] Relay setup failed (non-fatal):', err);
            }
        }

        console.log('[Main] TUI running');

    } catch (err) {
        console.error('[Main] TUI launch error:', err);
        terminalEl.innerHTML = `<pre style="color: #f7768e; padding: 1rem;">Error launching TUI:\n${err}</pre>`;
    }
})();
