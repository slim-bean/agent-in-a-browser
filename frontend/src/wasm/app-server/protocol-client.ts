/**
 * Typed Protocol Client for the App Server
 *
 * Wraps the raw JSON-based AppServerHandle with ergonomic, type-safe methods
 * for the most important app-server operations. Uses JSON-RPC-like envelope
 * format for requests/responses and typed event dispatch for notifications.
 */

import type { AppServerHandle, AppServerEvent } from './app-server-loader.js';

// ==========================================================================
// Request / Response Types
// ==========================================================================

/** Parameters for starting a new agent thread. */
export interface ThreadStartParams {
    model?: string;
    cwd?: string;
    instructions?: string;
}

/** Describes an agent thread. */
export interface Thread {
    id: string;
    name?: string;
    status: string;
    createdAt: number;
}

/** Response from thread/start. */
export interface ThreadStartResponse {
    thread: Thread;
    model: string;
    cwd: string;
}

/** A single user text input segment. */
export interface UserTextInput {
    type: 'text';
    text: string;
}

/** Parameters for starting a new turn within a thread. */
export interface TurnStartParams {
    threadId: string;
    input: UserTextInput[];
}

/** Describes a single turn in a thread. */
export interface Turn {
    id: string;
    status: string;
    createdAt: number;
}

/** Response from turn/start. */
export interface TurnStartResponse {
    turn: Turn;
}

/** Parameters for reading configuration. */
export interface ConfigReadParams {
    includeLayers?: boolean;
    cwd?: string;
}

// ==========================================================================
// Event Types (server -> client notifications)
// ==========================================================================

/** Emitted when a turn begins. */
export interface TurnStartedEvent {
    threadId: string;
    turn: Turn;
}

/** Streaming text delta from the agent. */
export interface AgentMessageDelta {
    threadId: string;
    turnId: string;
    itemId: string;
    delta: string;
}

/** A thread item (message, tool call, tool result, etc.). */
export interface ThreadItem {
    id: string;
    type: string;
    [key: string]: unknown;
}

/** Emitted when a new item starts within a turn. */
export interface ItemStartedEvent {
    threadId: string;
    turnId: string;
    item: ThreadItem;
}

/** Emitted when an item completes within a turn. */
export interface ItemCompletedEvent {
    threadId: string;
    turnId: string;
    item: ThreadItem;
}

/** Emitted when a turn finishes. */
export interface TurnCompletedEvent {
    threadId: string;
    turnId: string;
}

/** Approval request for a command the agent wants to execute. */
export interface CommandApprovalRequest {
    threadId: string;
    turnId: string;
    itemId: string;
    command?: string;
    cwd?: string;
    reason?: string;
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
// Client
// ==========================================================================

type EventHandler<T> = (event: T) => void;
type ApprovalHandler = (request: CommandApprovalRequest, requestId: string) => void;

/**
 * Typed protocol client wrapping an AppServerHandle.
 *
 * Provides ergonomic methods for the most common operations and a typed
 * event dispatch system for server-push notifications.
 */
export class AppServerClient {
    private handle: AppServerHandle;
    private requestId = 0;

    // Event handler registries (arrays to support multiple listeners)
    private turnStartedHandlers: EventHandler<TurnStartedEvent>[] = [];
    private agentMessageHandlers: EventHandler<AgentMessageDelta>[] = [];
    private itemStartedHandlers: EventHandler<ItemStartedEvent>[] = [];
    private itemCompletedHandlers: EventHandler<ItemCompletedEvent>[] = [];
    private turnCompletedHandlers: EventHandler<TurnCompletedEvent>[] = [];
    private approvalHandlers: ApprovalHandler[] = [];

    constructor(handle: AppServerHandle) {
        this.handle = handle;
        this.handle.onEvent((event) => this.dispatchEvent(event));
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
     * Send a JSON-RPC notification (fire-and-forget, no response expected).
     */
    private async sendNotification(method: string, params?: Record<string, unknown>): Promise<void> {
        const envelope: Omit<JsonRpcRequest, 'id'> & { method: string } = { method };
        if (params !== undefined) {
            (envelope as Record<string, unknown>)['params'] = params;
        }
        await this.handle.sendNotification(JSON.stringify(envelope));
    }

    // ------------------------------------------------------------------
    // Thread Operations
    // ------------------------------------------------------------------

    /** Start a new agent thread. */
    async startThread(params?: ThreadStartParams): Promise<ThreadStartResponse> {
        const rpcParams: Record<string, unknown> = {};
        if (params?.model !== undefined) rpcParams['model'] = params.model;
        if (params?.cwd !== undefined) rpcParams['cwd'] = params.cwd;
        if (params?.instructions !== undefined) rpcParams['instructions'] = params.instructions;

        return this.sendRequest<ThreadStartResponse>(
            'thread/start',
            Object.keys(rpcParams).length > 0 ? rpcParams : undefined,
        );
    }

    // ------------------------------------------------------------------
    // Turn Operations
    // ------------------------------------------------------------------

    /** Start a new turn (send user input) within a thread. */
    async startTurn(threadId: string, text: string): Promise<TurnStartResponse> {
        const input: UserTextInput[] = [{ type: 'text', text }];
        return this.sendRequest<TurnStartResponse>('turn/start', {
            threadId,
            input,
        });
    }

    /** Interrupt a running turn. */
    async interruptTurn(threadId: string, turnId: string): Promise<void> {
        await this.sendRequest<unknown>('turn/interrupt', { threadId, turnId });
    }

    // ------------------------------------------------------------------
    // Approval Operations
    // ------------------------------------------------------------------

    /** Approve a pending command execution request. */
    async approveCommand(requestId: string): Promise<void> {
        await this.handle.respondToServerRequest(
            requestId,
            JSON.stringify({ approved: true }),
        );
    }

    /** Deny a pending command execution request. */
    async denyCommand(requestId: string): Promise<void> {
        await this.handle.respondToServerRequest(
            requestId,
            JSON.stringify({ approved: false }),
        );
    }

    // ------------------------------------------------------------------
    // Config Operations
    // ------------------------------------------------------------------

    /** Read current configuration. */
    async readConfig(params?: ConfigReadParams): Promise<unknown> {
        const rpcParams: Record<string, unknown> = {};
        if (params?.includeLayers !== undefined) rpcParams['includeLayers'] = params.includeLayers;
        if (params?.cwd !== undefined) rpcParams['cwd'] = params.cwd;

        return this.sendRequest<unknown>(
            'config/read',
            Object.keys(rpcParams).length > 0 ? rpcParams : undefined,
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
    // Event Subscription
    // ------------------------------------------------------------------

    /** Register a handler for turn/started events. Returns an unsubscribe function. */
    onTurnStarted(handler: EventHandler<TurnStartedEvent>): () => void {
        this.turnStartedHandlers.push(handler);
        return () => {
            this.turnStartedHandlers = this.turnStartedHandlers.filter((h) => h !== handler);
        };
    }

    /** Register a handler for agent message deltas. Returns an unsubscribe function. */
    onAgentMessage(handler: EventHandler<AgentMessageDelta>): () => void {
        this.agentMessageHandlers.push(handler);
        return () => {
            this.agentMessageHandlers = this.agentMessageHandlers.filter((h) => h !== handler);
        };
    }

    /** Register a handler for item/started events. Returns an unsubscribe function. */
    onItemStarted(handler: EventHandler<ItemStartedEvent>): () => void {
        this.itemStartedHandlers.push(handler);
        return () => {
            this.itemStartedHandlers = this.itemStartedHandlers.filter((h) => h !== handler);
        };
    }

    /** Register a handler for item/completed events. Returns an unsubscribe function. */
    onItemCompleted(handler: EventHandler<ItemCompletedEvent>): () => void {
        this.itemCompletedHandlers.push(handler);
        return () => {
            this.itemCompletedHandlers = this.itemCompletedHandlers.filter((h) => h !== handler);
        };
    }

    /** Register a handler for turn/completed events. Returns an unsubscribe function. */
    onTurnCompleted(handler: EventHandler<TurnCompletedEvent>): () => void {
        this.turnCompletedHandlers.push(handler);
        return () => {
            this.turnCompletedHandlers = this.turnCompletedHandlers.filter((h) => h !== handler);
        };
    }

    /** Register a handler for command approval requests. Returns an unsubscribe function. */
    onApprovalRequired(handler: ApprovalHandler): () => void {
        this.approvalHandlers.push(handler);
        return () => {
            this.approvalHandlers = this.approvalHandlers.filter((h) => h !== handler);
        };
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
                break;
        }
    }

    private dispatchNotification(data: unknown): void {
        if (data === null || data === undefined || typeof data !== 'object') return;

        const notification = data as ServerNotification;
        const params = notification.params as Record<string, unknown> | undefined;
        if (!params) return;

        switch (notification.method) {
            case 'turn/started':
                for (const handler of this.turnStartedHandlers) {
                    handler(params as unknown as TurnStartedEvent);
                }
                break;
            case 'agent/message/delta':
                for (const handler of this.agentMessageHandlers) {
                    handler(params as unknown as AgentMessageDelta);
                }
                break;
            case 'item/started':
                for (const handler of this.itemStartedHandlers) {
                    handler(params as unknown as ItemStartedEvent);
                }
                break;
            case 'item/completed':
                for (const handler of this.itemCompletedHandlers) {
                    handler(params as unknown as ItemCompletedEvent);
                }
                break;
            case 'turn/completed':
                for (const handler of this.turnCompletedHandlers) {
                    handler(params as unknown as TurnCompletedEvent);
                }
                break;
        }
    }

    private dispatchServerRequest(data: unknown): void {
        if (data === null || data === undefined || typeof data !== 'object') return;

        const request = data as ServerRequest;
        const params = request.params as Record<string, unknown> | undefined;
        if (!params) return;

        switch (request.method) {
            case 'command/approval':
                for (const handler of this.approvalHandlers) {
                    handler(params as unknown as CommandApprovalRequest, request.id);
                }
                break;
        }
    }
}
