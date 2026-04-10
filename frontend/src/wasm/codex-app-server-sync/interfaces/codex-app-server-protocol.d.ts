/** @module Interface codex:app-server/protocol@0.1.0 **/
export function sendRequest(json: string): string;
export function sendNotification(json: string): void;
export function respondToServerRequest(requestId: string, resultJson: string): void;
export function failServerRequest(requestId: string, errorJson: string): void;
export function shutdown(): void;
