/**
 * Shell Execution Shim for Codex WASM Agent.
 *
 * Implements the host-side of `codex:agent/shell-exec@0.1.0`.
 * The WASM agent calls `exec(program, args, env, stdin, timeout_ms)`
 * and this shim routes it to a registered handler (set by the host).
 *
 * In the browser SharedWorker context, the handler delegates to the
 * MCP shell tool infrastructure. The host registers the handler via
 * `setExecHandler()` before creating any agent sessions.
 */

// ============================================================================
// Types matching the WIT interface
// ============================================================================

export interface ExecEnv {
    cwd: string;
    vars: [string, string][];
}

export interface ExecResult {
    exitCode: number;
    stdout: Uint8Array;
    stderr: Uint8Array;
}

/**
 * Handler function type. Must be async — JSPI will suspend the WASM stack.
 */
export type ExecHandler = (
    program: string,
    args: string[],
    env: ExecEnv,
    stdin: Uint8Array | undefined,
    timeoutMs: number | undefined,
) => Promise<ExecResult>;

// ============================================================================
// Registerable handler
// ============================================================================

let execHandler: ExecHandler | null = null;

/**
 * Register the shell execution handler.
 * Must be called before the WASM agent is created.
 */
export function setExecHandler(handler: ExecHandler): void {
    execHandler = handler;
}

/**
 * Get the current exec handler (for testing/introspection).
 */
export function getExecHandler(): ExecHandler | null {
    return execHandler;
}

// ============================================================================
// WIT-exported function
// ============================================================================

/**
 * Execute a command. Called by the WASM agent via WIT import.
 *
 * JCO maps `codex:tui/shell-exec@0.1.0#exec` to this function.
 * The JCO glue wraps the return in {tag:'ok', val} itself, so we
 * return the raw ExecResult. On error, we throw and JCO wraps in {tag:'err'}.
 */
export async function exec(
    program: string,
    args: string[],
    env: ExecEnv,
    stdin: Uint8Array | undefined,
    timeoutMs: number | undefined,
): Promise<ExecResult> {
    if (!execHandler) {
        throw new Error('No shell exec handler registered. Call setExecHandler() before creating an agent.');
    }

    return execHandler(program, args, env, stdin, timeoutMs);
}
