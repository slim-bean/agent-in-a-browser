// world root:component/root
export type * as CodexAppServerCredentialStore010 from './interfaces/codex-app-server-credential-store.js'; // import codex:app-server/credential-store@0.1.0
export type * as CodexAppServerEventSink010 from './interfaces/codex-app-server-event-sink.js'; // import codex:app-server/event-sink@0.1.0
export type * as CodexAppServerShellExec010 from './interfaces/codex-app-server-shell-exec.js'; // import codex:app-server/shell-exec@0.1.0
export type * as CodexAppServerShellPty010 from './interfaces/codex-app-server-shell-pty.js'; // import codex:app-server/shell-pty@0.1.0
export type * as CodexAppServerWebsocket010 from './interfaces/codex-app-server-websocket.js'; // import codex:app-server/websocket@0.1.0
export type * as HostBrowserActions010 from './interfaces/host-browser-actions.js'; // import host:browser/actions@0.1.0
export type * as HostBrowserAudio010 from './interfaces/host-browser-audio.js'; // import host:browser/audio@0.1.0
export type * as HostConsoleLogging010 from './interfaces/host-console-logging.js'; // import host:console/logging@0.1.0
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
export * as protocol from './interfaces/codex-app-server-protocol.js'; // export codex:app-server/protocol@0.1.0
export function start(): number;
export function pushAuthCallback(method: string, path: string, headers: Array<[string, string]>, body: Uint8Array): void;

export const $init: Promise<void>;
