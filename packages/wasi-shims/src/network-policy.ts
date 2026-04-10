/**
 * Network Policy Engine for HTTP/WebSocket Requests.
 *
 * Evaluates outgoing network requests against allow/deny/prompt lists to
 * enforce security policy before the request is made. Part of Phase 2
 * security enforcement.
 */

export type Decision = 'allow' | 'prompt' | 'deny';

export class NetworkPolicy {
    // Domains that are always allowed (LLM API providers, essential services)
    private static readonly SAFE_DOMAINS = new Set([
        'api.openai.com',
        'api.anthropic.com',
        'api.together.xyz',
        'api.groq.com',
        'api.mistral.ai',
        'api.deepseek.com',
        'api.fireworks.ai',
        'api.cohere.com',
        'openrouter.ai',
        'generativelanguage.googleapis.com',
        'localhost',
        '127.0.0.1',
        '[::1]',
    ]);

    // Domains that should always be denied (common exfiltration targets)
    private static readonly DENIED_DOMAINS = new Set([
        'webhook.site',
        'requestbin.com',
        'pipedream.com',
        'ngrok.io',
        'ngrok-free.app',
        'burpcollaborator.net',
    ]);

    // Session-approved domains (approved by user during this session)
    private sessionApprovals = new Map<string, Decision>();

    evaluate(url: string, _method: string): Decision {
        let hostname: string;
        try {
            const parsed = new URL(url);
            hostname = parsed.hostname;
        } catch {
            // If the URL can't be parsed, prompt for safety
            return 'prompt';
        }

        // Check deny list first
        if (NetworkPolicy.DENIED_DOMAINS.has(hostname)) {
            return 'deny';
        }

        // Check session approvals
        const sessionDecision = this.sessionApprovals.get(hostname);
        if (sessionDecision) {
            return sessionDecision;
        }

        // Check safe list (exact match)
        if (NetworkPolicy.SAFE_DOMAINS.has(hostname)) {
            return 'allow';
        }

        // Allow wasm:// scheme (local MCP routing)
        try {
            const parsed = new URL(url);
            if (parsed.protocol === 'wasm:') {
                return 'allow';
            }
        } catch {
            // Already handled above
        }

        // Everything else needs approval
        return 'prompt';
    }

    approveForSession(url: string): void {
        try {
            const parsed = new URL(url);
            this.sessionApprovals.set(parsed.hostname, 'allow');
        } catch {
            // Cannot parse URL; ignore
        }
    }

    reset(): void {
        this.sessionApprovals.clear();
    }
}

// Shared singleton instance — HTTP and WebSocket share approval state
const sharedNetworkPolicy = new NetworkPolicy();

export function getNetworkPolicy(): NetworkPolicy {
    return sharedNetworkPolicy;
}
