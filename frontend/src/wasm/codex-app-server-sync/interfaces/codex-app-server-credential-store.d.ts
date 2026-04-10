/** @module Interface codex:app-server/credential-store@0.1.0 **/
export function load(service: string, account: string): string | undefined;
export function save(service: string, account: string, value: string): void;
export function deleteCredential(service: string, account: string): boolean;
