/**
 * Browser Actions Shim.
 *
 * Implements the host-side of `host:browser/actions@0.1.0`.
 * The WASM component calls `open-url(url)` and this shim routes it
 * to the main thread via BroadcastChannel, which calls
 * window.open(url, '_blank') to open a new tab.
 *
 * Works from SharedWorker, dedicated Worker, or main thread contexts.
 */

// ============================================================================
// BroadcastChannel for Worker → Main Thread communication
// ============================================================================

const CHANNEL_NAME = 'browser-actions';

/**
 * Handler function type. Must be async — JSPI will suspend the WASM stack.
 */
export type OpenUrlHandler = (url: string) => Promise<void>;

// ============================================================================
// Registerable handler (with BroadcastChannel default)
// ============================================================================

let openUrlHandler: OpenUrlHandler | null = null;

/**
 * Register a custom open-url handler (overrides the default BroadcastChannel).
 * Must be called before the WASM module runs.
 */
export function setOpenUrlHandler(handler: OpenUrlHandler): void {
    openUrlHandler = handler;
}

/**
 * Default handler: send via BroadcastChannel.
 * The main thread must call `listenForOpenUrl()` to receive these.
 */
async function defaultHandler(url: string): Promise<void> {
    const channel = new BroadcastChannel(CHANNEL_NAME);
    channel.postMessage({ type: 'open-url', url });
    channel.close();
}

// ============================================================================
// Main-thread listener
// ============================================================================

/**
 * Start listening for open-url requests on the main thread.
 * Call this once from the main thread (e.g., main-tui.ts).
 * Opens URLs in a new tab via window.open().
 */
export function listenForOpenUrl(): void {
    const channel = new BroadcastChannel(CHANNEL_NAME);
    channel.onmessage = (e: MessageEvent) => {
        if (e.data?.type === 'open-url' && typeof e.data.url === 'string') {
            window.open(e.data.url, '_blank');
        }
    };
}

// ============================================================================
// WIT-exported function
// ============================================================================

/**
 * Open a URL in a new browser tab.
 *
 * JCO maps `host:browser/actions@0.1.0#open-url` to this function.
 * JCO wraps the return in {tag:'ok'}/{tag:'err'}, so we return void
 * on success or throw on error.
 */
export async function openUrl(url: string): Promise<void> {
    const handler = openUrlHandler || defaultHandler;
    return handler(url);
}
