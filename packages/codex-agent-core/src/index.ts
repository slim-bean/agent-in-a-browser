/**
 * @tjfontaine/codex-agent-core
 *
 * Codex-based AI agent for web applications.
 * Uses OpenAI's Codex CLI engine compiled to WASM.
 *
 * @example
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
 * // Streaming
 * for await (const event of agent.send('Hello!')) {
 *   if (event.type === 'chunk') console.log(event.text);
 * }
 *
 * // One-shot
 * const response = await agent.prompt('Summarize');
 *
 * agent.destroy();
 * ```
 */

export { CodexAgent } from './agent.js';
export type {
    AgentConfig,
    AgentEvent,
    Message,
    MessageRole,
    ToolResultData,
    TaskInfo,
    TaskUpdateInfo,
    TaskCompleteInfo,
} from './types.js';
