/**
 * WebSocket implementation for codex:tui/websocket WIT interface.
 *
 * Bridges the Codex TUI WASM WebSocket calls to the browser's native
 * WebSocket API. Used for the OpenAI Responses WebSocket transport.
 *
 * Auth headers are passed as WebSocket subprotocols since the browser
 * WebSocket API doesn't support custom headers. OpenAI supports:
 *   openai-insecure-api-key.<token>
 *   openai-beta.responses-v1
 */

// ============================================================================
// Connection Management
// ============================================================================

interface WSConnection {
    ws: WebSocket;
    /** Queued incoming text messages */
    messageQueue: string[];
    closed: boolean;
    error: string | null;
    /** Resolve function for blocking recv (JSPI mode) */
    readResolve: ((value: void) => void) | null;
}

let nextHandle = 1;
const connections = new Map<number, WSConnection>();

// ============================================================================
// Exported WIT Functions
// ============================================================================

/**
 * Open a WebSocket connection with optional subprotocols.
 * Returns a Promise (JSPI-suspending) that resolves to a handle.
 */
export function connect(url: string, protocols: string[]): Promise<number> {
    const handle = nextHandle++;

    return new Promise<number>((resolve, reject) => {
        let ws: WebSocket;
        try {
            ws = protocols.length > 0
                ? new WebSocket(url, protocols)
                : new WebSocket(url);
        } catch (err) {
            reject(`WebSocket constructor failed: ${err}`);
            return;
        }

        const conn: WSConnection = {
            ws,
            messageQueue: [],
            closed: false,
            error: null,
            readResolve: null,
        };

        connections.set(handle, conn);

        ws.onopen = () => {
            console.log(`[websocket-impl] connected handle=${handle} url=${url}`);
            resolve(handle);
        };

        ws.onmessage = (event: MessageEvent) => {
            if (typeof event.data === 'string') {
                conn.messageQueue.push(event.data);
            } else {
                // Binary message — convert to string for the text-only WIT interface
                const text = typeof event.data === 'string'
                    ? event.data
                    : new TextDecoder().decode(
                        event.data instanceof ArrayBuffer
                            ? new Uint8Array(event.data)
                            : new Uint8Array(event.data as ArrayBuffer)
                    );
                conn.messageQueue.push(text);
            }

            // Wake up any blocked recv
            if (conn.readResolve) {
                conn.readResolve();
                conn.readResolve = null;
            }
        };

        ws.onclose = (event: CloseEvent) => {
            console.log(`[websocket-impl] closed handle=${handle} code=${event.code}`);
            conn.closed = true;

            // Wake up any blocked recv so it returns None
            if (conn.readResolve) {
                conn.readResolve();
                conn.readResolve = null;
            }
        };

        ws.onerror = (event: Event) => {
            const errorMsg = `WebSocket error: ${event.type}`;
            console.error(`[websocket-impl] error handle=${handle}:`, errorMsg);
            conn.error = errorMsg;
            conn.closed = true;

            // If we haven't connected yet, reject the connect promise
            // If already connected, wake up any blocked recv
            if (conn.readResolve) {
                conn.readResolve();
                conn.readResolve = null;
            }
        };

        // Handle connection failure during handshake
        const origOnError = ws.onerror;
        ws.onerror = (event: Event) => {
            ws.onerror = origOnError;
            conn.closed = true;
            conn.error = 'Connection failed';
            reject('WebSocket connection failed');
        };

        // Once connected, restore normal error handler
        const origOnOpen = ws.onopen;
        ws.onopen = (event: Event) => {
            ws.onerror = (ev: Event) => {
                conn.error = `WebSocket error: ${ev.type}`;
                conn.closed = true;
                if (conn.readResolve) {
                    conn.readResolve();
                    conn.readResolve = null;
                }
            };
            // Call the resolve handler
            console.log(`[websocket-impl] connected handle=${handle} url=${url}`);
            resolve(handle);
        };
    });
}

/**
 * Send a text message on the WebSocket.
 */
export function send(handle: number, data: string): void {
    const conn = connections.get(handle);
    if (!conn) {
        throw new Error(`WebSocket handle ${handle} not found`);
    }
    if (conn.closed) {
        throw new Error(`WebSocket handle ${handle} is closed`);
    }
    conn.ws.send(data);
}

/**
 * Receive the next text message from the WebSocket.
 * Returns a Promise (JSPI-suspending) that resolves with the message string,
 * or undefined when the connection is closed.
 */
export function recv(handle: number): string | undefined | Promise<string | undefined> {
    const conn = connections.get(handle);
    if (!conn) {
        return undefined;
    }

    // If we have queued messages, return immediately
    if (conn.messageQueue.length > 0) {
        return conn.messageQueue.shift()!;
    }

    // If closed and no messages, return undefined (maps to None in Rust)
    if (conn.closed) {
        return undefined;
    }

    // Wait for next message via JSPI suspension
    return new Promise<string | undefined>((resolve) => {
        conn.readResolve = () => {
            if (conn.messageQueue.length > 0) {
                resolve(conn.messageQueue.shift()!);
            } else {
                // Closed with no remaining messages
                resolve(undefined);
            }
        };
    });
}

/**
 * Close the WebSocket connection.
 */
export function close(handle: number): void {
    const conn = connections.get(handle);
    if (conn) {
        if (!conn.closed) {
            conn.closed = true;
            try {
                conn.ws.close();
            } catch {
                // Ignore close errors
            }
        }
        connections.delete(handle);
    }
}

/**
 * Check if the connection is closed.
 */
export function isClosed(handle: number): boolean {
    const conn = connections.get(handle);
    if (!conn) {
        return true;
    }
    return conn.closed;
}
