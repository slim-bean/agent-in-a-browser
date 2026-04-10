/** @module Interface codex:app-server/shell-exec@0.1.0 **/
export function exec(program: string, args: Array<string>, env: ExecEnv, stdin: Uint8Array | undefined, timeoutMs: number | undefined): ExecResult;
export interface ExecEnv {
  cwd: string,
  vars: Array<[string, string]>,
}
export interface ExecResult {
  exitCode: number,
  stdout: Uint8Array,
  stderr: Uint8Array,
}
