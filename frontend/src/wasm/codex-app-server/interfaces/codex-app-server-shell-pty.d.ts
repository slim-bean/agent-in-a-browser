/** @module Interface codex:app-server/shell-pty@0.1.0 **/
export function start(params: PtyStartParams): PtyStartResult;
export function read(processId: string, afterSeq: bigint | undefined, maxBytes: number | undefined, waitMs: bigint | undefined): PtyReadResult;
export function write(processId: string, data: Uint8Array): PtyWriteResult;
export function terminate(processId: string): void;
export function pollWake(processId: string): bigint;
export interface PtyStartParams {
  processId: string,
  argv: Array<string>,
  cwd: string,
  env: Array<[string, string]>,
  tty: boolean,
}
export interface PtyStartResult {
  processId: string,
}
export interface PtyOutputChunk {
  seq: bigint,
  data: Uint8Array,
}
export interface PtyReadResult {
  chunks: Array<PtyOutputChunk>,
  nextSeq: bigint,
  exited: boolean,
  exitCode?: number,
  closed: boolean,
  failure?: string,
}
/**
 * # Variants
 * 
 * ## `"accepted"`
 * 
 * ## `"unknown-process"`
 * 
 * ## `"stdin-closed"`
 * 
 * ## `"starting"`
 */
export type WriteStatus = 'accepted' | 'unknown-process' | 'stdin-closed' | 'starting';
export interface PtyWriteResult {
  status: WriteStatus,
}
