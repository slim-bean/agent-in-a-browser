// world root:component/root
export interface McpServerConfig {
  url: string,
  name?: string,
}
export interface ProviderInfo {
  id: string,
  name: string,
  defaultBaseUrl?: string,
}
export interface ModelInfo {
  id: string,
  name: string,
}
export interface AgentConfig {
  provider: string,
  model: string,
  apiKey: string,
  baseUrl?: string,
  preamble?: string,
  preambleOverride?: string,
  mcpServers?: Array<McpServerConfig>,
  maxTurns?: number,
}
/**
* # Variants
* 
* ## `"user"`
* 
* ## `"assistant"`
*/
export type MessageRole = 'user' | 'assistant';
export interface Message {
  role: MessageRole,
  content: string,
}
export interface ToolResultData {
  name: string,
  output: string,
  isError: boolean,
}
export interface TaskInfo {
  id: string,
  name: string,
  description: string,
}
export interface TaskUpdateInfo {
  id: string,
  status: string,
  progress?: number,
}
export interface TaskCompleteInfo {
  id: string,
  success: boolean,
  output?: string,
}
export interface ModelLoadingProgress {
  text: string,
  progress: number,
}
export type AgentEvent = AgentEventStreamStart | AgentEventStreamChunk | AgentEventStreamComplete | AgentEventStreamError | AgentEventToolCall | AgentEventToolResult | AgentEventPlanGenerated | AgentEventTaskStart | AgentEventTaskUpdate | AgentEventTaskComplete | AgentEventModelLoading | AgentEventReady;
export interface AgentEventStreamStart {
  tag: 'stream-start',
}
export interface AgentEventStreamChunk {
  tag: 'stream-chunk',
  val: string,
}
export interface AgentEventStreamComplete {
  tag: 'stream-complete',
  val: string,
}
export interface AgentEventStreamError {
  tag: 'stream-error',
  val: string,
}
export interface AgentEventToolCall {
  tag: 'tool-call',
  val: string,
}
export interface AgentEventToolResult {
  tag: 'tool-result',
  val: ToolResultData,
}
export interface AgentEventPlanGenerated {
  tag: 'plan-generated',
  val: string,
}
export interface AgentEventTaskStart {
  tag: 'task-start',
  val: TaskInfo,
}
export interface AgentEventTaskUpdate {
  tag: 'task-update',
  val: TaskUpdateInfo,
}
export interface AgentEventTaskComplete {
  tag: 'task-complete',
  val: TaskCompleteInfo,
}
export interface AgentEventModelLoading {
  tag: 'model-loading',
  val: ModelLoadingProgress,
}
export interface AgentEventReady {
  tag: 'ready',
}
export type AgentHandle = number;
export type * as CodexAgentShellExec010 from './interfaces/codex-agent-shell-exec.js'; // import codex:agent/shell-exec@0.1.0
export type * as WasiCliEnvironment029 from './interfaces/wasi-cli-environment.js'; // import wasi:cli/environment@0.2.9
export type * as WasiCliExit029 from './interfaces/wasi-cli-exit.js'; // import wasi:cli/exit@0.2.9
export type * as WasiCliStderr029 from './interfaces/wasi-cli-stderr.js'; // import wasi:cli/stderr@0.2.9
export type * as WasiCliStdin029 from './interfaces/wasi-cli-stdin.js'; // import wasi:cli/stdin@0.2.9
export type * as WasiCliStdout029 from './interfaces/wasi-cli-stdout.js'; // import wasi:cli/stdout@0.2.9
export type * as WasiCliTerminalInput029 from './interfaces/wasi-cli-terminal-input.js'; // import wasi:cli/terminal-input@0.2.9
export type * as WasiCliTerminalOutput029 from './interfaces/wasi-cli-terminal-output.js'; // import wasi:cli/terminal-output@0.2.9
export type * as WasiCliTerminalStderr029 from './interfaces/wasi-cli-terminal-stderr.js'; // import wasi:cli/terminal-stderr@0.2.9
export type * as WasiCliTerminalStdin029 from './interfaces/wasi-cli-terminal-stdin.js'; // import wasi:cli/terminal-stdin@0.2.9
export type * as WasiCliTerminalStdout029 from './interfaces/wasi-cli-terminal-stdout.js'; // import wasi:cli/terminal-stdout@0.2.9
export type * as WasiClocksMonotonicClock029 from './interfaces/wasi-clocks-monotonic-clock.js'; // import wasi:clocks/monotonic-clock@0.2.9
export type * as WasiClocksWallClock029 from './interfaces/wasi-clocks-wall-clock.js'; // import wasi:clocks/wall-clock@0.2.9
export type * as WasiFilesystemPreopens029 from './interfaces/wasi-filesystem-preopens.js'; // import wasi:filesystem/preopens@0.2.9
export type * as WasiFilesystemTypes029 from './interfaces/wasi-filesystem-types.js'; // import wasi:filesystem/types@0.2.9
export type * as WasiHttpOutgoingHandler029 from './interfaces/wasi-http-outgoing-handler.js'; // import wasi:http/outgoing-handler@0.2.9
export type * as WasiHttpTypes029 from './interfaces/wasi-http-types.js'; // import wasi:http/types@0.2.9
export type * as WasiIoError029 from './interfaces/wasi-io-error.js'; // import wasi:io/error@0.2.9
export type * as WasiIoPoll029 from './interfaces/wasi-io-poll.js'; // import wasi:io/poll@0.2.9
export type * as WasiIoStreams029 from './interfaces/wasi-io-streams.js'; // import wasi:io/streams@0.2.9
export type * as WasiRandomInsecureSeed029 from './interfaces/wasi-random-insecure-seed.js'; // import wasi:random/insecure-seed@0.2.9
export type * as WasiRandomRandom029 from './interfaces/wasi-random-random.js'; // import wasi:random/random@0.2.9
export function create(config: AgentConfig): Promise<AgentHandle>;
export function destroy(handle: AgentHandle): Promise<void>;
export function sendMessage(handle: AgentHandle, message: string): void;
export function poll(handle: AgentHandle): Promise<AgentEvent | undefined>;
export function cancel(handle: AgentHandle): Promise<void>;
export function plan(handle: AgentHandle, message: string): Promise<void>;
export function execute(handle: AgentHandle): Promise<void>;
export function getHistory(handle: AgentHandle): Array<Message>;
export function clearHistory(handle: AgentHandle): void;
export function listProviders(): Array<ProviderInfo>;
export function listModels(providerId: string): Array<ModelInfo>;
export function fetchModels(providerId: string, apiKey: string, baseUrl: string | undefined): Array<ModelInfo>;
