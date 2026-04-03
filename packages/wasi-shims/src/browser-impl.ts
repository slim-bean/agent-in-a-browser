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
 * Show a clickable toast when window.open() is popup-blocked.
 * The toast provides a link the user can click (a real user gesture)
 * which opens the URL in a new tab.
 */
function showOpenUrlToast(url: string): void {
    // Remove any existing toast
    const existing = document.getElementById('open-url-toast');
    if (existing) existing.remove();

    const toast = document.createElement('div');
    toast.id = 'open-url-toast';
    toast.style.cssText = [
        'position:fixed', 'bottom:24px', 'left:50%', 'transform:translateX(-50%)',
        'background:#1e293b', 'color:#e2e8f0', 'padding:12px 20px',
        'border-radius:8px', 'box-shadow:0 4px 12px rgba(0,0,0,0.4)',
        'z-index:999999', 'font-family:system-ui,sans-serif', 'font-size:14px',
        'display:flex', 'align-items:center', 'gap:12px',
    ].join(';');

    const label = document.createElement('span');
    label.textContent = 'Popup blocked —';

    const link = document.createElement('a');
    link.href = url;
    link.target = '_blank';
    link.rel = 'noopener';
    link.textContent = 'click here to open';
    link.style.cssText = 'color:#60a5fa;text-decoration:underline;cursor:pointer';
    link.addEventListener('click', () => {
        toast.remove();
    });

    const dismiss = document.createElement('button');
    dismiss.textContent = '✕';
    dismiss.style.cssText = [
        'background:none', 'border:none', 'color:#94a3b8', 'cursor:pointer',
        'font-size:16px', 'padding:0 0 0 4px', 'line-height:1',
    ].join(';');
    dismiss.addEventListener('click', () => toast.remove());

    toast.appendChild(label);
    toast.appendChild(link);
    toast.appendChild(dismiss);
    document.body.appendChild(toast);

    // Auto-dismiss after 30 seconds
    setTimeout(() => toast.remove(), 30_000);
}

/**
 * Start listening for open-url requests on the main thread.
 * Call this once from the main thread (e.g., main-tui.ts).
 * Opens URLs in a new tab via window.open(). If the popup is blocked
 * (no user activation), shows a clickable toast as fallback.
 */
export function listenForOpenUrl(): void {
    const channel = new BroadcastChannel(CHANNEL_NAME);
    channel.onmessage = (e: MessageEvent) => {
        if (e.data?.type === 'open-url' && typeof e.data.url === 'string') {
            const win = window.open(e.data.url, '_blank');
            if (!win) {
                showOpenUrlToast(e.data.url);
            }
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
