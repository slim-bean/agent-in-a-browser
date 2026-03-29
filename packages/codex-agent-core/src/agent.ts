/**
 * @tjfontaine/codex-agent-core
 *
 * CodexAgent - Embeddable AI agent for web applications.
 * Uses OpenAI's Codex CLI engine compiled to WASM.
 *
 * Usage:
 * ```typescript
 * import { CodexAgent } from '@tjfontaine/codex-agent-core';
 *
 * const agent = new CodexAgent({
 *   provider: 'openai',
 *   model: 'o4-mini',
 *   apiKey: process.env.OPENAI_API_KEY!,
 * });
 *
 * await agent.initialize();
 *
 * // Streaming mode
 * for await (const event of agent.send('Hello!')) {
 *   if (event.type === 'chunk') console.log(event.text);
 * }
 *
 * // One-shot mode
 * const response = await agent.prompt('Summarize');
 * console.log(response);
 *
 * agent.destroy();
 * ```
 */

import type { AgentConfig, AgentEvent, Message, WasmAgentConfig, WasmAgentEvent, WasmMessage, AgentHandle } from './types.js';

// The WASM module will be loaded dynamically
let wasmModule: WasmModule | null = null;

interface WasmModule {
    create(config: WasmAgentConfig): AgentHandle;
    destroy(handle: AgentHandle): void;
    sendMessage(handle: AgentHandle, message: string): void;
    poll(handle: AgentHandle): WasmAgentEvent | undefined;
    cancel(handle: AgentHandle): void;
    plan(handle: AgentHandle, message: string): void;
    execute(handle: AgentHandle): void;
    getHistory(handle: AgentHandle): WasmMessage[];
    clearHistory(handle: AgentHandle): void;
}

/**
 * Load the WASM module
 */
async function loadWasmModule(): Promise<WasmModule> {
    if (wasmModule) return wasmModule;

    // Initialize WASI shims before loading WASM (required for JSPI async operations)
    console.log('[CodexAgent] Initializing WASI shims...');
    const shims = await import('@tjfontaine/wasi-shims') as { initFilesystem: () => Promise<void> };
    await shims.initFilesystem();
    console.log('[CodexAgent] WASI shims initialized');

    // Dynamic import of the jco-transpiled module
    const mod = await import('./wasm/codex-wasm-agent.js');
    wasmModule = mod as unknown as WasmModule;
    return wasmModule;
}

/**
 * Convert WASM event to TypeScript event
 */
function mapEvent(event: WasmAgentEvent): AgentEvent {
    switch (event.tag) {
        case 'stream-start':
            return { type: 'stream-start' };
        case 'stream-chunk':
            return { type: 'chunk', text: event.val };
        case 'stream-complete':
            return { type: 'complete', text: event.val };
        case 'stream-error':
            return { type: 'error', error: event.val };
        case 'tool-call':
            return { type: 'tool-call', toolName: event.val };
        case 'tool-result':
            return { type: 'tool-result', data: event.val };
        case 'plan-generated':
            return { type: 'plan-generated', plan: event.val };
        case 'task-start':
            return { type: 'task-start', task: event.val };
        case 'task-update':
            return { type: 'task-update', update: event.val };
        case 'task-complete':
            return { type: 'task-complete', result: event.val };
        case 'ready':
            return { type: 'ready' };
        default:
            console.warn(`[CodexAgent] Unknown event type:`, event);
            return { type: 'ready' };
    }
}

/**
 * Convert config to WASM format
 */
function toWasmConfig(config: AgentConfig): WasmAgentConfig {
    return {
        provider: config.provider,
        model: config.model,
        apiKey: config.apiKey,
        baseUrl: config.baseUrl,
        preamble: config.preamble,
        preambleOverride: config.preambleOverride,
        mcpServers: config.mcpServers?.map(s => ({ url: s.url, name: s.name })),
        maxTurns: config.maxTurns,
    };
}

/**
 * Convert WASM message to TypeScript message
 */
function mapMessage(msg: WasmMessage): Message {
    return {
        role: msg.role,
        content: msg.content,
    };
}

/**
 * CodexAgent - Main class for interacting with the AI agent
 */
export class CodexAgent {
    private handle: AgentHandle | null = null;
    private wasm: WasmModule | null = null;
    private _isInitialized = false;

    constructor(private config: AgentConfig) { }

    /**
     * Initialize the agent (loads WASM module and creates a session)
     */
    async initialize(): Promise<void> {
        if (this._isInitialized) return;

        this.wasm = await loadWasmModule();
        this.handle = await this.wasm.create(toWasmConfig(this.config));
        this._isInitialized = true;
    }

    /**
     * Check if the agent is initialized
     */
    get isInitialized(): boolean {
        return this._isInitialized;
    }

    /**
     * Send a message and get an async iterator of events
     */
    async *send(message: string): AsyncGenerator<AgentEvent> {
        if (!this.handle || !this.wasm) {
            throw new Error('Agent not initialized. Call initialize() first.');
        }

        await this.wasm.sendMessage(this.handle, message);

        // Poll for events
        while (true) {
            const event = await this.wasm.poll(this.handle);

            if (!event) {
                // No event available, yield to event loop
                await new Promise(r => setTimeout(r, 10));
                continue;
            }

            const mapped = mapEvent(event);
            yield mapped;

            // Stop polling on terminal events
            if (mapped.type === 'complete' || mapped.type === 'error' || mapped.type === 'ready') {
                break;
            }
        }
    }

    /**
     * Send a message and wait for the complete response
     */
    async prompt(message: string): Promise<string> {
        let result = '';

        for await (const event of this.send(message)) {
            if (event.type === 'chunk') {
                result += event.text;
            } else if (event.type === 'complete') {
                return event.text;
            } else if (event.type === 'error') {
                throw new Error(event.error);
            }
        }

        return result;
    }

    /**
     * Cancel the current stream
     */
    cancel(): void {
        if (this.handle && this.wasm) {
            this.wasm.cancel(this.handle);
        }
    }

    /**
     * Get conversation history
     */
    getHistory(): Message[] {
        if (!this.handle || !this.wasm) {
            return [];
        }
        return this.wasm.getHistory(this.handle).map(mapMessage);
    }

    /**
     * Clear conversation history
     */
    clearHistory(): void {
        if (this.handle && this.wasm) {
            this.wasm.clearHistory(this.handle);
        }
    }

    /**
     * Destroy the agent and release resources
     */
    destroy(): void {
        if (this.handle && this.wasm) {
            this.wasm.destroy(this.handle);
            this.handle = null;
        }
        this._isInitialized = false;
    }
}
