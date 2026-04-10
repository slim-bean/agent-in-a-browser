/**
 * Command Policy Engine for Shell Execution.
 *
 * Evaluates shell commands against allow/deny/prompt lists to enforce
 * security policy before execution. Part of Phase 1 security enforcement.
 */

export type Decision = 'allow' | 'prompt' | 'deny';

export class CommandPolicy {
    // Safe read-only commands that never need approval
    private static readonly SAFE_COMMANDS = new Set([
        'ls', 'cat', 'head', 'tail', 'grep', 'rg', 'find', 'pwd', 'echo',
        'wc', 'sort', 'uniq', 'diff', 'file', 'stat', 'which', 'whoami',
        'date', 'env', 'printenv', 'uname', 'hostname', 'id',
        'git', 'node', 'python', 'python3', 'deno', 'bun',
        'cargo', 'rustc', 'npm', 'npx', 'pnpm', 'yarn',
        'tsc', 'eslint', 'prettier', 'jest', 'vitest',
        'tree', 'less', 'more', 'strings', 'hexdump', 'xxd',
        'basename', 'dirname', 'realpath', 'readlink',
        'true', 'false', 'test', '[',
    ]);

    // Commands that should always be denied (network exfiltration risk from shell)
    private static readonly DENIED_COMMANDS = new Set([
        'curl', 'wget', 'nc', 'ncat', 'netcat', 'socat',
        'ssh', 'scp', 'sftp', 'rsync',
        'telnet', 'ftp',
    ]);

    // Git subcommands that are safe (read-only)
    private static readonly SAFE_GIT_SUBCOMMANDS = new Set([
        'status', 'log', 'diff', 'show', 'branch', 'tag',
        'remote', 'stash', 'blame', 'shortlog', 'describe',
        'ls-files', 'ls-tree', 'cat-file', 'rev-parse',
        'config', 'help', 'version',
    ]);

    // Session-approved commands (approved by user during this session)
    private sessionApprovals = new Map<string, Decision>();

    evaluate(program: string, args: string[]): Decision {
        const basename = program.split('/').pop() ?? program;

        // Check deny list first
        if (CommandPolicy.DENIED_COMMANDS.has(basename)) {
            return 'deny';
        }

        // Check session approvals
        const key = this.commandKey(basename, args);
        const sessionDecision = this.sessionApprovals.get(key);
        if (sessionDecision) {
            return sessionDecision;
        }

        // Check safe list
        if (CommandPolicy.SAFE_COMMANDS.has(basename)) {
            // For git, check subcommand
            if (basename === 'git' && args.length > 0) {
                const subcommand = args[0];
                if (CommandPolicy.SAFE_GIT_SUBCOMMANDS.has(subcommand)) {
                    return 'allow';
                }
                return 'prompt'; // Non-read-only git commands need approval
            }
            return 'allow';
        }

        // Everything else needs approval
        return 'prompt';
    }

    approveForSession(program: string, args: string[]): void {
        const basename = program.split('/').pop() ?? program;
        const key = this.commandKey(basename, args);
        this.sessionApprovals.set(key, 'allow');
    }

    private commandKey(basename: string, args: string[]): string {
        // Key by program name + first arg (for git subcommands etc)
        return args.length > 0 ? `${basename}:${args[0]}` : basename;
    }

    reset(): void {
        this.sessionApprovals.clear();
    }
}
