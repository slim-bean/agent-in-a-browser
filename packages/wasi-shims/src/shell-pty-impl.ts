/**
 * Shell PTY Shim for Codex WASM Agent.
 *
 * Implements the host-side of `codex:tui/shell-pty@0.1.0`.
 * The WASM agent calls start/read/write/terminate/pollWake and this shim
 * routes them to a registered handler (set by the host).
 *
 * In the browser context, the handler manages persistent shell sessions
 * backed by the MCP server's ShellEnv instances.
 */

// ============================================================================
// Types matching the WIT interface
// ============================================================================

export interface PtyStartParams {
    processId: string;
    argv: string[];
    cwd: string;
    env: [string, string][];
    tty: boolean;
}

export interface PtyStartResult {
    processId: string;
}

export interface PtyOutputChunk {
    seq: bigint;
    data: Uint8Array;
}

export interface PtyReadResult {
    chunks: PtyOutputChunk[];
    nextSeq: bigint;
    exited: boolean;
    exitCode: number | undefined;
    closed: boolean;
    failure: string | undefined;
}

export type WriteStatus = 'accepted' | 'unknown-process' | 'stdin-closed' | 'starting';

export interface PtyWriteResult {
    status: WriteStatus;
}

/**
 * Handler interface for PTY operations.
 */
export interface PtyHandler {
    start(params: PtyStartParams): Promise<PtyStartResult>;
    read(
        processId: string,
        afterSeq: bigint | undefined,
        maxBytes: number | undefined,
        waitMs: bigint | undefined,
    ): Promise<PtyReadResult>;
    write(processId: string, data: Uint8Array): Promise<PtyWriteResult>;
    terminate(processId: string): Promise<void>;
    pollWake(processId: string): Promise<bigint>;
}

// ============================================================================
// Registerable handler
// ============================================================================

let ptyHandler: PtyHandler | null = null;

/**
 * Register the PTY handler.
 * Must be called before the WASM agent is created.
 */
export function setPtyHandler(handler: PtyHandler): void {
    ptyHandler = handler;
}

/**
 * Get the current PTY handler (for testing/introspection).
 */
export function getPtyHandler(): PtyHandler | null {
    return ptyHandler;
}

// ============================================================================
// WIT-exported functions
// ============================================================================

function ensureHandler(): PtyHandler {
    if (!ptyHandler) {
        throw new Error(
            'No PTY handler registered. Call setPtyHandler() before creating an agent.',
        );
    }
    return ptyHandler;
}

/**
 * Start a new persistent shell session.
 */
export async function start(params: PtyStartParams): Promise<PtyStartResult> {
    return ensureHandler().start(params);
}

/**
 * Read output from a session.
 */
export async function read(
    processId: string,
    afterSeq: bigint | undefined,
    maxBytes: number | undefined,
    waitMs: bigint | undefined,
): Promise<PtyReadResult> {
    return ensureHandler().read(processId, afterSeq, maxBytes, waitMs);
}

/**
 * Write data to a session's stdin.
 */
export async function write(
    processId: string,
    data: Uint8Array,
): Promise<PtyWriteResult> {
    return ensureHandler().write(processId, data);
}

/**
 * Terminate a session.
 */
export async function terminate(processId: string): Promise<void> {
    return ensureHandler().terminate(processId);
}

/**
 * Poll for new output availability.
 */
export async function pollWake(processId: string): Promise<bigint> {
    return ensureHandler().pollWake(processId);
}
