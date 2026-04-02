/**
 * Browser Clipboard Shim.
 *
 * Implements the host-side of `host:browser/clipboard@0.1.0`.
 * The WASM component calls `read-text()` / `write-text(text)` and this shim
 * routes them to navigator.clipboard.
 *
 * In JSPI mode, the async clipboard API works because JSPI suspends the WASM
 * stack while the Promise resolves. In sync mode (Safari), we fall back to
 * in-memory storage since the Clipboard API requires async and there is no
 * synchronous alternative.
 */

// ============================================================================
// In-memory fallback for sync mode / restricted contexts
// ============================================================================

let inMemoryClipboard = '';

// ============================================================================
// WIT-exported functions
// ============================================================================

/**
 * Read text from the system clipboard.
 *
 * JCO maps `host:browser/clipboard@0.1.0#read-text` to this function.
 * In JSPI mode, the async navigator.clipboard.readText() call suspends
 * the WASM stack. In sync mode or when clipboard API is unavailable,
 * falls back to in-memory storage.
 */
export async function readText(): Promise<string> {
    if (typeof navigator !== 'undefined' && navigator.clipboard?.readText) {
        try {
            return await navigator.clipboard.readText();
        } catch (_e: unknown) {
            // Clipboard API may throw due to permissions or focus requirements.
            // Fall back to in-memory.
            return inMemoryClipboard;
        }
    }
    // SharedWorker or non-secure context — no clipboard API available.
    return inMemoryClipboard;
}

/**
 * Write text to the system clipboard.
 *
 * JCO maps `host:browser/clipboard@0.1.0#write-text` to this function.
 * In JSPI mode, the async navigator.clipboard.writeText() call suspends
 * the WASM stack. Always updates in-memory fallback as well.
 */
export async function writeText(text: string): Promise<void> {
    // Always update in-memory so it is available if read falls back.
    inMemoryClipboard = text;

    if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
        try {
            await navigator.clipboard.writeText(text);
            return;
        } catch (_e: unknown) {
            // Clipboard API may throw due to permissions or focus requirements.
            // In-memory fallback already updated above.
            return;
        }
    }
    // SharedWorker or non-secure context — in-memory fallback already set.
}
