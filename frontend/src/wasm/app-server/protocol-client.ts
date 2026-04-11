/**
 * Typed Protocol Client for the App Server
 *
 * Wraps the raw JSON-based AppServerHandle with ergonomic, type-safe methods
 * for the full app-server protocol. Uses JSON-RPC envelope format for
 * requests/responses and typed event dispatch for notifications and
 * server-initiated requests.
 *
 * Data types are imported from @tjfontaine/codex-protocol-types (the
 * canonical, generated schema). Only the RPC wrapper types and client class
 * are defined locally.
 */

import type { AppServerHandle, AppServerEvent } from './app-server-loader.js';

// ==========================================================================
// Canonical type imports from generated schema
// ==========================================================================

import type {
    // Core data types
    ThreadItem,
    Turn,
    Thread,
    GitInfo,
    FileUpdateChange,
    PatchChangeKind,
    CommandExecutionStatus,
    McpToolCallStatus,
    CommandAction,
    CommandExecutionSource,
    PatchApplyStatus,
    UserInput,
    ThreadTokenUsage,
    TokenUsageBreakdown,
    TurnError,
    ThreadStatus,
    TurnStatus,
    HookRunSummary,

    // Request/Response types
    ThreadStartParams,
    ThreadStartResponse,
    TurnStartParams,
    TurnStartResponse,
    ThreadListParams,
    ThreadListResponse,
    ThreadReadParams,
    ThreadReadResponse,
    ThreadResumeParams,
    ThreadResumeResponse,
    ThreadArchiveParams,
    ThreadRollbackParams,
    TurnInterruptParams,
    TurnSteerParams,
    ConfigReadParams,
    ConfigReadResponse,
    ModelListParams,
    ModelListResponse,
    SkillsListParams,
    SkillsListResponse,
    ListMcpServerStatusParams,
    ListMcpServerStatusResponse,

    // Notification types
    TurnStartedNotification,
    TurnCompletedNotification,
    AgentMessageDeltaNotification,
    ItemStartedNotification,
    ItemCompletedNotification,
    PlanDeltaNotification,
    ReasoningTextDeltaNotification,
    ReasoningSummaryTextDeltaNotification,
    CommandExecutionOutputDeltaNotification,
    FileChangeOutputDeltaNotification,
    McpToolCallProgressNotification,
    ItemGuardianApprovalReviewStartedNotification,
    ItemGuardianApprovalReviewCompletedNotification,
    HookStartedNotification,
    HookCompletedNotification,
    ErrorNotification,
    ThreadStartedNotification,
    ThreadStatusChangedNotification,
    ThreadClosedNotification,
    ThreadNameUpdatedNotification,
    ThreadTokenUsageUpdatedNotification,
    ContextCompactedNotification,
    TurnDiffUpdatedNotification,
    TurnPlanUpdatedNotification,
    AccountRateLimitsUpdatedNotification,
    McpServerStatusUpdatedNotification,
    CommandExecOutputDeltaNotification,
    AccountLoginCompletedNotification,
    AccountUpdatedNotification,
    LoginAccountParams,
    LoginAccountResponse,
    CancelLoginAccountParams,
    CancelLoginAccountResponse,
    Account,
    GetAccountParams,
    GetAccountResponse,

    // Server request types
    CommandExecutionRequestApprovalParams,
    FileChangeRequestApprovalParams,
    PermissionsRequestApprovalParams,
    ToolRequestUserInputParams,
    McpServerElicitationRequestParams,
} from '@tjfontaine/codex-protocol-types';

import type {
    // Root-level types (not in v2)
    GetAuthStatusResponse,
    GetConversationSummaryResponse,
    GetConversationSummaryParams,
    FuzzyFileSearchParams,
    FuzzyFileSearchResponse,
    ApplyPatchApprovalParams,
    AuthMode,
    PlanType,
} from '@tjfontaine/codex-protocol-types';

// ==========================================================================
// Re-export all imported canonical types
// ==========================================================================

export type {
    // Core data types
    ThreadItem,
    Turn,
    Thread,
    GitInfo,
    FileUpdateChange,
    PatchChangeKind,
    CommandExecutionStatus,
    McpToolCallStatus,
    CommandAction,
    CommandExecutionSource,
    PatchApplyStatus,
    UserInput,
    ThreadTokenUsage,
    TokenUsageBreakdown,
    TurnError,
    ThreadStatus,
    TurnStatus,
    HookRunSummary,

    // Request/Response types
    ThreadStartParams,
    ThreadStartResponse,
    TurnStartParams,
    TurnStartResponse,
    ThreadListParams,
    ThreadListResponse,
    ThreadReadParams,
    ThreadReadResponse,
    ThreadResumeParams,
    ThreadResumeResponse,
    ThreadArchiveParams,
    ThreadRollbackParams,
    TurnInterruptParams,
    TurnSteerParams,
    ConfigReadParams,
    ConfigReadResponse,
    ModelListParams,
    ModelListResponse,
    SkillsListParams,
    SkillsListResponse,
    ListMcpServerStatusParams,
    ListMcpServerStatusResponse,

    // Notification types
    TurnStartedNotification,
    TurnCompletedNotification,
    AgentMessageDeltaNotification,
    ItemStartedNotification,
    ItemCompletedNotification,
    PlanDeltaNotification,
    ReasoningTextDeltaNotification,
    ReasoningSummaryTextDeltaNotification,
    CommandExecutionOutputDeltaNotification,
    FileChangeOutputDeltaNotification,
    McpToolCallProgressNotification,
    ItemGuardianApprovalReviewStartedNotification,
    ItemGuardianApprovalReviewCompletedNotification,
    HookStartedNotification,
    HookCompletedNotification,
    ErrorNotification,
    ThreadStartedNotification,
    ThreadStatusChangedNotification,
    ThreadClosedNotification,
    ThreadNameUpdatedNotification,
    ThreadTokenUsageUpdatedNotification,
    ContextCompactedNotification,
    TurnDiffUpdatedNotification,
    TurnPlanUpdatedNotification,
    AccountRateLimitsUpdatedNotification,
    McpServerStatusUpdatedNotification,
    CommandExecOutputDeltaNotification,
    AccountLoginCompletedNotification,
    AccountUpdatedNotification,
    LoginAccountParams,
    LoginAccountResponse,
    CancelLoginAccountParams,
    CancelLoginAccountResponse,
    Account,
    GetAccountParams,
    GetAccountResponse,

    // Server request types
    CommandExecutionRequestApprovalParams,
    FileChangeRequestApprovalParams,
    PermissionsRequestApprovalParams,
    ToolRequestUserInputParams,
    McpServerElicitationRequestParams,

    // Root-level types
    GetAuthStatusResponse,
    GetConversationSummaryResponse,
    GetConversationSummaryParams,
    FuzzyFileSearchParams,
    FuzzyFileSearchResponse,
    ApplyPatchApprovalParams,
    AuthMode,
    PlanType,
};

// ==========================================================================
// ThreadItem variant extraction
// ==========================================================================

/** Extract named types from the ThreadItem discriminated union for pattern matching. */
export type UserMessageItem = Extract<ThreadItem, { type: 'userMessage' }>;
export type AgentMessageItem = Extract<ThreadItem, { type: 'agentMessage' }>;
export type PlanItem = Extract<ThreadItem, { type: 'plan' }>;
export type ReasoningItem = Extract<ThreadItem, { type: 'reasoning' }>;
export type CommandExecutionItem = Extract<ThreadItem, { type: 'commandExecution' }>;
export type FileChangeItem = Extract<ThreadItem, { type: 'fileChange' }>;
export type McpToolCallItem = Extract<ThreadItem, { type: 'mcpToolCall' }>;
export type DynamicToolCallItem = Extract<ThreadItem, { type: 'dynamicToolCall' }>;
export type WebSearchItem = Extract<ThreadItem, { type: 'webSearch' }>;
export type ImageViewItem = Extract<ThreadItem, { type: 'imageView' }>;
export type ContextCompactionItem = Extract<ThreadItem, { type: 'contextCompaction' }>;
export type HookPromptItem = Extract<ThreadItem, { type: 'hookPrompt' }>;

// ==========================================================================
// Notification aliases (UI uses shorter names)
// ==========================================================================

export type TurnStartedEvent = TurnStartedNotification;
export type TurnCompletedEvent = TurnCompletedNotification;
export type AgentMessageDelta = AgentMessageDeltaNotification;
export type ItemStartedEvent = ItemStartedNotification;
export type ItemCompletedEvent = ItemCompletedNotification;
export type GuardianReviewStartedNotification = ItemGuardianApprovalReviewStartedNotification;
export type GuardianReviewCompletedNotification = ItemGuardianApprovalReviewCompletedNotification;

// ==========================================================================
// Server request aliases (UI uses shorter names)
// ==========================================================================

export type CommandApprovalRequest = CommandExecutionRequestApprovalParams;
export type FileChangeApprovalRequest = FileChangeRequestApprovalParams;
export type PermissionsApprovalRequest = PermissionsRequestApprovalParams;
export type ToolUserInputRequest = ToolRequestUserInputParams;
export type PatchApprovalRequest = ApplyPatchApprovalParams;
export type McpElicitationRequest = McpServerElicitationRequestParams;

// ==========================================================================
// Legacy alias
// ==========================================================================

export type TokenUsage = ThreadTokenUsage;

// ==========================================================================
// Notification & Server Request Maps
// ==========================================================================

/** Maps notification method strings to their payload types. */
export interface NotificationMap {
    'turn/started': TurnStartedNotification;
    'turn/completed': TurnCompletedNotification;
    'item/agentMessage/delta': AgentMessageDeltaNotification;
    'item/started': ItemStartedNotification;
    'item/completed': ItemCompletedNotification;
    'item/plan/delta': PlanDeltaNotification;
    'item/reasoning/textDelta': ReasoningTextDeltaNotification;
    'item/reasoning/summaryTextDelta': ReasoningSummaryTextDeltaNotification;
    'item/commandExecution/outputDelta': CommandExecutionOutputDeltaNotification;
    'item/fileChange/outputDelta': FileChangeOutputDeltaNotification;
    'item/mcpToolCall/progress': McpToolCallProgressNotification;
    'item/autoApprovalReview/started': ItemGuardianApprovalReviewStartedNotification;
    'item/autoApprovalReview/completed': ItemGuardianApprovalReviewCompletedNotification;
    'hook/started': HookStartedNotification;
    'hook/completed': HookCompletedNotification;
    'turn/diff/updated': TurnDiffUpdatedNotification;
    'turn/plan/updated': TurnPlanUpdatedNotification;
    'thread/started': ThreadStartedNotification;
    'thread/status/changed': ThreadStatusChangedNotification;
    'thread/closed': ThreadClosedNotification;
    'thread/name/updated': ThreadNameUpdatedNotification;
    'thread/tokenUsage/updated': ThreadTokenUsageUpdatedNotification;
    'thread/compacted': ContextCompactedNotification;
    'error': ErrorNotification;
    'account/rateLimits/updated': AccountRateLimitsUpdatedNotification;
    'mcpServer/startupStatus/updated': McpServerStatusUpdatedNotification;
    'command/exec/outputDelta': CommandExecOutputDeltaNotification;
    'account/login/completed': AccountLoginCompletedNotification;
    'account/updated': AccountUpdatedNotification;
}

/** Maps server request method strings to their payload types. */
export interface ServerRequestMap {
    'item/commandExecution/requestApproval': CommandExecutionRequestApprovalParams;
    'item/fileChange/requestApproval': FileChangeRequestApprovalParams;
    'item/permissions/requestApproval': PermissionsRequestApprovalParams;
    'item/tool/requestUserInput': ToolRequestUserInputParams;
    'applyPatchApproval': ApplyPatchApprovalParams;
    'mcpServer/elicitation/request': McpServerElicitationRequestParams;
}

// ==========================================================================
// JSON-RPC Envelope Types (internal)
// ==========================================================================

interface JsonRpcRequest {
    id: string;
    method: string;
    params?: Record<string, unknown>;
}

interface JsonRpcSuccessResponse {
    result: unknown;
}

interface JsonRpcErrorResponse {
    error: {
        code: number;
        message: string;
        data?: unknown;
    };
}

type JsonRpcResponse = JsonRpcSuccessResponse | JsonRpcErrorResponse;

/** Structured representation of a server-push notification. */
interface ServerNotification {
    method: string;
    params?: Record<string, unknown>;
}

/** Structured representation of a server-initiated request (e.g. approval). */
interface ServerRequest {
    id: string;
    method: string;
    params?: Record<string, unknown>;
}

// ==========================================================================
// Error
// ==========================================================================

/** Error thrown when the server returns a JSON-RPC error response. */
export class AppServerError extends Error {
    readonly code: number;
    readonly data: unknown;

    constructor(code: number, message: string, data?: unknown) {
        super(message);
        this.name = 'AppServerError';
        this.code = code;
        this.data = data;
    }
}

// ==========================================================================
// Type Guards
// ==========================================================================

function isErrorResponse(resp: JsonRpcResponse): resp is JsonRpcErrorResponse {
    return 'error' in resp && resp.error !== undefined;
}

// ==========================================================================
// Handler Types
// ==========================================================================

type EventHandler<T> = (event: T) => void;

/** Legacy approval handler type (for backward compat). */
type ApprovalHandler = (request: CommandApprovalRequest, requestId: string) => void;

/** Server request handler receives params, a respond callback, and a reject callback. */
type ServerRequestHandler<T> = (
    params: T,
    respond: (result: unknown) => void,
    reject: (error: { code: number; message: string }) => void,
) => void;

// ==========================================================================
// Client
// ==========================================================================

/**
 * Typed protocol client wrapping an AppServerHandle.
 *
 * Provides ergonomic methods for the full app-server protocol and a typed
 * event dispatch system for server-push notifications and server requests.
 */
export class AppServerClient {
    private handle: AppServerHandle;
    private requestId = 0;

    /** Generic notification handler registry keyed by method string. */
    private notificationHandlers = new Map<string, Array<(data: unknown) => void>>();

    /** Generic server request handler registry keyed by method string. */
    private serverRequestHandlers = new Map<string, Array<(data: unknown, respond: (result: unknown) => void, reject: (error: { code: number; message: string }) => void) => void>>();

    /** Legacy approval handlers (backward compat). */
    private legacyApprovalHandlers: ApprovalHandler[] = [];

    constructor(handle: AppServerHandle) {
        this.handle = handle;
        this.handle.onEvent((event) => this.dispatchEvent(event));
    }

    // ------------------------------------------------------------------
    // Generic Event Subscription
    // ------------------------------------------------------------------

    /**
     * Register a typed handler for a notification event.
     * Returns an unsubscribe function.
     */
    on<K extends keyof NotificationMap>(
        event: K,
        handler: EventHandler<NotificationMap[K]>,
    ): () => void {
        const key = event as string;
        let handlers = this.notificationHandlers.get(key);
        if (!handlers) {
            handlers = [];
            this.notificationHandlers.set(key, handlers);
        }
        // Cast is safe: we maintain type safety at the on() boundary
        const wrappedHandler = handler as (data: unknown) => void;
        handlers.push(wrappedHandler);
        return () => {
            const arr = this.notificationHandlers.get(key);
            if (arr) {
                const idx = arr.indexOf(wrappedHandler);
                if (idx !== -1) {
                    arr.splice(idx, 1);
                }
            }
        };
    }

    /**
     * Register a typed handler for a server-initiated request.
     * The handler receives (params, respond, reject) where respond/reject
     * call through to the underlying handle.
     * Returns an unsubscribe function.
     */
    onServerRequest<K extends keyof ServerRequestMap>(
        method: K,
        handler: ServerRequestHandler<ServerRequestMap[K]>,
    ): () => void {
        const key = method as string;
        let handlers = this.serverRequestHandlers.get(key);
        if (!handlers) {
            handlers = [];
            this.serverRequestHandlers.set(key, handlers);
        }
        const wrappedHandler = handler as (data: unknown, respond: (result: unknown) => void, reject: (error: { code: number; message: string }) => void) => void;
        handlers.push(wrappedHandler);
        return () => {
            const arr = this.serverRequestHandlers.get(key);
            if (arr) {
                const idx = arr.indexOf(wrappedHandler);
                if (idx !== -1) {
                    arr.splice(idx, 1);
                }
            }
        };
    }

    // ------------------------------------------------------------------
    // Legacy Event Subscription (backward compat)
    // ------------------------------------------------------------------

    /** Register a handler for turn/started events. Returns an unsubscribe function. */
    onTurnStarted(handler: EventHandler<TurnStartedEvent>): () => void {
        return this.on('turn/started', handler);
    }

    /** Register a handler for agent message deltas. Returns an unsubscribe function. */
    onAgentMessage(handler: EventHandler<AgentMessageDelta>): () => void {
        return this.on('item/agentMessage/delta', handler);
    }

    /** Register a handler for item/started events. Returns an unsubscribe function. */
    onItemStarted(handler: EventHandler<ItemStartedEvent>): () => void {
        return this.on('item/started', handler);
    }

    /** Register a handler for item/completed events. Returns an unsubscribe function. */
    onItemCompleted(handler: EventHandler<ItemCompletedEvent>): () => void {
        return this.on('item/completed', handler);
    }

    /** Register a handler for turn/completed events. Returns an unsubscribe function. */
    onTurnCompleted(handler: EventHandler<TurnCompletedEvent>): () => void {
        return this.on('turn/completed', handler);
    }

    /**
     * Register a handler for command approval requests.
     * This is a legacy convenience method. For typed server request handling,
     * prefer `onServerRequest('item/commandExecution/requestApproval', ...)`.
     * Returns an unsubscribe function.
     */
    onApprovalRequired(handler: ApprovalHandler): () => void {
        this.legacyApprovalHandlers.push(handler);
        return () => {
            const idx = this.legacyApprovalHandlers.indexOf(handler);
            if (idx !== -1) {
                this.legacyApprovalHandlers.splice(idx, 1);
            }
        };
    }

    // ------------------------------------------------------------------
    // Request Helpers
    // ------------------------------------------------------------------

    /**
     * Send a typed JSON-RPC request and return the parsed result.
     * Throws AppServerError on error responses.
     */
    private async sendRequest<T>(method: string, params?: Record<string, unknown>): Promise<T> {
        const id = String(++this.requestId);
        const envelope: JsonRpcRequest = { id, method };
        if (params !== undefined) {
            envelope.params = params;
        }

        const rawResponse = await this.handle.sendRequest(JSON.stringify(envelope));
        const response: JsonRpcResponse = JSON.parse(rawResponse) as JsonRpcResponse;

        if (isErrorResponse(response)) {
            throw new AppServerError(
                response.error.code,
                response.error.message,
                response.error.data,
            );
        }

        return response.result as T;
    }

    /**
     * Build a params object from optional fields, omitting undefined values.
     * Returns undefined if no fields are set.
     */
    private static buildParams(fields: Record<string, unknown>): Record<string, unknown> | undefined {
        const result: Record<string, unknown> = {};
        let hasFields = false;
        for (const [key, value] of Object.entries(fields)) {
            if (value !== undefined) {
                result[key] = value;
                hasFields = true;
            }
        }
        return hasFields ? result : undefined;
    }

    // ------------------------------------------------------------------
    // Thread Operations
    // ------------------------------------------------------------------

    /** Start a new agent thread. */
    async startThread(params?: ThreadStartParams): Promise<ThreadStartResponse> {
        return this.sendRequest<ThreadStartResponse>(
            'thread/start',
            params as Record<string, unknown> | undefined,
        );
    }

    /** List existing threads. */
    async listThreads(params?: ThreadListParams): Promise<ThreadListResponse> {
        return this.sendRequest<ThreadListResponse>(
            'thread/list',
            params as Record<string, unknown> | undefined,
        );
    }

    /** Read a thread by ID. */
    async readThread(threadId: string, options?: { includeTurns?: boolean }): Promise<ThreadReadResponse> {
        return this.sendRequest<ThreadReadResponse>(
            'thread/read',
            AppServerClient.buildParams({
                threadId,
                includeTurns: options?.includeTurns,
            }),
        );
    }

    /** Resume an existing thread. */
    async resumeThread(threadId: string, options?: { model?: string }): Promise<ThreadResumeResponse> {
        return this.sendRequest<ThreadResumeResponse>(
            'thread/resume',
            AppServerClient.buildParams({
                threadId,
                model: options?.model,
            }),
        );
    }

    /** Archive a thread. */
    async archiveThread(threadId: string): Promise<void> {
        await this.sendRequest<unknown>('thread/archive', { threadId });
    }

    /** Set a thread's display name. */
    async setThreadName(threadId: string, name: string): Promise<void> {
        await this.sendRequest<unknown>('thread/setName', { threadId, name });
    }

    /** Rollback a thread to a specific turn. */
    async rollbackThread(threadId: string, turnId: string): Promise<void> {
        await this.sendRequest<unknown>('thread/rollback', { threadId, turnId });
    }

    // ------------------------------------------------------------------
    // Turn Operations
    // ------------------------------------------------------------------

    /** Start a new turn (send user input) within a thread. */
    async startTurn(threadId: string, text: string): Promise<TurnStartResponse> {
        const input: UserInput[] = [{ type: 'text', text, text_elements: [] }];
        return this.sendRequest<TurnStartResponse>('turn/start', {
            threadId,
            input,
        });
    }

    /** Steer an in-progress turn with additional guidance. */
    async steerTurn(threadId: string, turnId: string, text: string): Promise<void> {
        await this.sendRequest<unknown>('turn/steer', { threadId, turnId, text });
    }

    /** Interrupt a running turn. */
    async interruptTurn(threadId: string, turnId: string): Promise<void> {
        await this.sendRequest<unknown>('turn/interrupt', { threadId, turnId });
    }

    // ------------------------------------------------------------------
    // Config Operations
    // ------------------------------------------------------------------

    /** Read current configuration. */
    async readConfig(params?: ConfigReadParams): Promise<ConfigReadResponse> {
        return this.sendRequest<ConfigReadResponse>(
            'config/read',
            params as Record<string, unknown> | undefined,
        );
    }

    // ------------------------------------------------------------------
    // Models & Account
    // ------------------------------------------------------------------

    /** List available models. */
    async listModels(): Promise<ModelListResponse> {
        return this.sendRequest<ModelListResponse>('model/list');
    }

    /** Read current account information. */
    async readAccount(options?: { refreshToken?: boolean }): Promise<GetAccountResponse> {
        return this.sendRequest<GetAccountResponse>('account/read', {
            refreshToken: options?.refreshToken ?? false,
        });
    }

    /** Login with an API key. */
    async loginWithApiKey(apiKey: string): Promise<LoginAccountResponse> {
        return this.sendRequest<LoginAccountResponse>('account/login/start', {
            type: 'apiKey',
            apiKey,
        });
    }

    /** Start OAuth login flow. Returns loginId and authUrl for browser redirect. */
    async loginWithOAuth(): Promise<LoginAccountResponse> {
        return this.sendRequest<LoginAccountResponse>('account/login/start', {
            type: 'chatgpt',
        });
    }

    /** Cancel an in-progress login. */
    async cancelLogin(loginId: string): Promise<CancelLoginAccountResponse> {
        return this.sendRequest<CancelLoginAccountResponse>('account/login/cancel', {
            loginId,
        });
    }

    /** Logout and clear credentials. */
    async logout(): Promise<void> {
        await this.sendRequest<unknown>('account/logout');
    }

    /** Check authentication status. */
    async getAuthStatus(): Promise<GetAuthStatusResponse> {
        return this.sendRequest<GetAuthStatusResponse>('getAuthStatus', {
            includeToken: false,
            refreshToken: false,
        });
    }

    // ------------------------------------------------------------------
    // Skills & MCP
    // ------------------------------------------------------------------

    /** List available skills. */
    async listSkills(): Promise<SkillsListResponse> {
        return this.sendRequest<SkillsListResponse>('skills/list');
    }

    /** List MCP server statuses. */
    async listMcpServerStatus(): Promise<ListMcpServerStatusResponse> {
        return this.sendRequest<ListMcpServerStatusResponse>('mcpServer/status/list');
    }

    // ------------------------------------------------------------------
    // Utilities
    // ------------------------------------------------------------------

    /** Get a summary of the conversation in a thread. */
    async getConversationSummary(threadId: string): Promise<GetConversationSummaryResponse> {
        return this.sendRequest<GetConversationSummaryResponse>('conversation/summary', { threadId });
    }

    /** Fuzzy file search in the working directory. */
    async fuzzyFileSearch(query: string, cwd?: string): Promise<FuzzyFileSearchResponse> {
        return this.sendRequest<FuzzyFileSearchResponse>(
            'file/fuzzySearch',
            AppServerClient.buildParams({ query, cwd }),
        );
    }

    // ------------------------------------------------------------------
    // Approval / Server Request Responses
    // ------------------------------------------------------------------

    /** Approve a pending command execution request (legacy convenience). */
    async approveCommand(requestId: string): Promise<void> {
        await this.handle.respondToServerRequest(
            requestId,
            JSON.stringify({ approved: true }),
        );
    }

    /** Deny a pending command execution request (legacy convenience). */
    async denyCommand(requestId: string): Promise<void> {
        await this.handle.respondToServerRequest(
            requestId,
            JSON.stringify({ approved: false }),
        );
    }

    /** Respond to a server-initiated request with a result. */
    async respondToRequest(requestId: string, result: unknown): Promise<void> {
        await this.handle.respondToServerRequest(
            requestId,
            JSON.stringify(result),
        );
    }

    /** Fail a server-initiated request with an error. */
    async failRequest(requestId: string, error: { code: number; message: string }): Promise<void> {
        await this.handle.failServerRequest(
            requestId,
            JSON.stringify(error),
        );
    }

    // ------------------------------------------------------------------
    // Shutdown
    // ------------------------------------------------------------------

    /** Gracefully shut down the app server. */
    async shutdown(): Promise<void> {
        await this.handle.shutdown();
    }

    // ------------------------------------------------------------------
    // Event Dispatch (internal)
    // ------------------------------------------------------------------

    private dispatchEvent(event: AppServerEvent): void {
        switch (event.type) {
            case 'notification':
                this.dispatchNotification(event.data);
                break;
            case 'request':
                this.dispatchServerRequest(event.data);
                break;
            case 'lagged':
                console.warn(
                    `[AppServerClient] Event stream lagged, skipped ${event.skipped ?? 0} events`,
                );
                break;
            case 'error':
                console.error(`[AppServerClient] Server error: ${event.message ?? 'unknown'}`);
                // Also dispatch as an error notification for any listeners
                this.emitNotification('error', { message: event.message ?? 'unknown' });
                break;
        }
    }

    private dispatchNotification(data: unknown): void {
        if (data === null || data === undefined || typeof data !== 'object') return;

        const notification = data as ServerNotification;
        const params = notification.params as Record<string, unknown> | undefined;
        if (!params) return;

        this.emitNotification(notification.method, params);
    }

    /** Emit a notification to all registered handlers for the given method. */
    private emitNotification(method: string, params: unknown): void {
        const handlers = this.notificationHandlers.get(method);
        if (handlers) {
            for (const handler of handlers) {
                handler(params);
            }
        }
    }

    private dispatchServerRequest(data: unknown): void {
        if (data === null || data === undefined || typeof data !== 'object') return;

        const request = data as ServerRequest;
        const params = request.params as Record<string, unknown> | undefined;
        if (!params) return;

        // Check for typed server request handlers first
        const handlers = this.serverRequestHandlers.get(request.method);
        if (handlers && handlers.length > 0) {
            const respond = (result: unknown): void => {
                void this.handle.respondToServerRequest(
                    request.id,
                    JSON.stringify(result),
                );
            };
            const reject = (error: { code: number; message: string }): void => {
                void this.handle.failServerRequest(
                    request.id,
                    JSON.stringify(error),
                );
            };
            for (const handler of handlers) {
                handler(params, respond, reject);
            }
            return;
        }

        // Fall back to legacy approval handlers for command approval requests
        if (request.method === 'item/commandExecution/requestApproval') {
            for (const handler of this.legacyApprovalHandlers) {
                handler(params as unknown as CommandApprovalRequest, request.id);
            }
        }
    }
}
