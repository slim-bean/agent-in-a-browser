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
 *
 * Phase 1 security: Commands are evaluated against a CommandPolicy before
 * execution. Denied commands are blocked, unknown commands require user
 * approval via an ApprovalHandler.
 */

import { CommandPolicy } from './command-policy.js';

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
// Approval handler for policy-prompted commands
// ============================================================================

export type ApprovalDecision = 'allow' | 'deny' | 'allow-session';

export type ApprovalHandler = (
    program: string,
    args: string[],
    env: ExecEnv,
) => Promise<ApprovalDecision>;

let approvalHandler: ApprovalHandler | null = null;
const commandPolicy = new CommandPolicy();

export function setApprovalHandler(handler: ApprovalHandler): void {
    approvalHandler = handler;
}

export function getCommandPolicy(): CommandPolicy {
    return commandPolicy;
}

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

    // Phase 1 security: evaluate command against policy before execution
    const decision = commandPolicy.evaluate(program, args);

    if (decision === 'deny') {
        return {
            exitCode: 126, // "Command cannot execute"
            stdout: new Uint8Array(),
            stderr: new TextEncoder().encode(`Command denied by policy: ${program}`),
        };
    }

    if (decision === 'prompt') {
        if (!approvalHandler) {
            // No approval handler = deny by default (safe default)
            return {
                exitCode: 126,
                stdout: new Uint8Array(),
                stderr: new TextEncoder().encode(
                    `Command requires approval but no handler registered: ${program}`,
                ),
            };
        }
        const approval = await approvalHandler(program, args, env);
        if (approval === 'deny') {
            return {
                exitCode: 126,
                stdout: new Uint8Array(),
                stderr: new TextEncoder().encode(`Command denied by user: ${program}`),
            };
        }
        if (approval === 'allow-session') {
            commandPolicy.approveForSession(program, args);
        }
    }

    // Only reach here if allowed — proceed to execHandler
    return execHandler(program, args, env, stdin, timeoutMs);
}
