type EventHandler = (json: string) => void;

let handler: EventHandler | null = null;

/**
 * Register a handler for app-server events pushed from WASM.
 */
export function setEventHandler(h: EventHandler): void {
    handler = h;
}

/**
 * Called by WASM via the event-sink WIT import when the app-server emits an event.
 * JCO maps `codex:app-server/event-sink@0.1.0#emit-event` to this function.
 */
export function emitEvent(json: string): void {
    if (handler) {
        handler(json);
    }
}
