/** @module Interface codex:tui/websocket@0.1.0 **/
export function connect(url: string, protocols: Array<string>): number;
export function send(handle: number, data: string): void;
export function recv(handle: number): string | undefined;
export function close(handle: number): void;
export function isClosed(handle: number): boolean;
