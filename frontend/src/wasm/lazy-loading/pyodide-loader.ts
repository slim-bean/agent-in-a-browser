/**
 * Pyodide Loader — Adapts Pyodide (CPython in WASM) to the CommandModule interface.
 *
 * Unlike Rust/Go WASM modules, Pyodide is a pre-built Emscripten WASM binary
 * with its own JS glue. We load it via its npm API and wrap it to match the
 * same spawn()/listCommands() interface used by all other lazy-loaded modules.
 *
 * The Pyodide instance is cached as a singleton — Python startup is expensive
 * (~5-10s) so we keep the interpreter alive across invocations.
 *
 * OPFS integration: Our custom Pyodide build uses WasmFS with the OPFS backend
 * mounted at /home/user and /lib/python3.12/site-packages via a C-level
 * wasmfs_before_preload() hook. No JS-side mount or sync is needed — WasmFS
 * handles persistence natively through the OPFS backend.
 */

import type {
    CommandModule,
    CommandHandle,
    ExecEnv,
    InputStream,
    OutputStream,
} from '@tjfontaine/wasm-loader';
import { closeAllHandles } from '@tjfontaine/wasi-shims/directory-tree.js';

// Pyodide types (loaded dynamically)
interface PyodideInterface {
    runPythonAsync(code: string): Promise<unknown>;
    loadPackage(names: string | string[]): Promise<void>;
    setStdout(options: { batched: (text: string) => void }): void;
    setStderr(options: { batched: (text: string) => void }): void;
    FS: {
        readFile(path: string, opts?: { encoding: string }): string;
        writeFile(path: string, data: string | Uint8Array): void;
        mkdir(path: string): void;
        stat(path: string): unknown;
        chdir(path: string): void;
        readdir(path: string): string[];
    };
    globals: {
        get(name: string): unknown;
    };
}

type LoadPyodideFn = (options: {
    indexURL: string;
    env?: Record<string, string>;
    stdout?: (text: string) => void;
    stderr?: (text: string) => void;
}) => Promise<PyodideInterface>;

// Singleton Pyodide instance
let pyodideInstance: PyodideInterface | null = null;
let pyodideLoading: Promise<PyodideInterface> | null = null;

const OPFS_MOUNT_PATH = '/home/user';

/**
 * Initialize or return the cached Pyodide instance.
 */
async function getPyodide(): Promise<PyodideInterface> {
    if (pyodideInstance) return pyodideInstance;
    if (pyodideLoading) return pyodideLoading;

    pyodideLoading = (async () => {
        console.log('[PyodideLoader] Loading Pyodide...');
        const startTime = performance.now();

        // Dynamic import of Pyodide — served from /pyodide/ as static assets.
        // The UMD bundle sets globalThis.loadPyodide as a side effect;
        // the ESM build exports it directly. Handle both.
        const pyodideUrl = '/pyodide/pyodide.mjs';
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const pyodideMod: any = await import(/* @vite-ignore */ pyodideUrl);
        const loadPyodide: LoadPyodideFn = pyodideMod.loadPyodide
            ?? (globalThis as any).loadPyodide;

        let py;
        try {
            py = await loadPyodide({
                indexURL: '/pyodide/',
                // Set HOME to /home/user — matches the OPFS mount point and
                // the shell's working directory convention.
                env: { HOME: '/home/user' },
                // With WasmFS, Pyodide's device-based stream redirection fails.
                // Use stdout/stderr callbacks which hook into Module.print/printErr.
                stdout: (msg: string) => { console.log('[Python stdout]', msg); },
                stderr: (msg: string) => { console.warn('[Python stderr]', msg); },
            });
        } catch (e: unknown) {
            const msg = e instanceof Error ? e.message : String(e);
            const errno = (e as any)?.errno ?? 'unknown';
            console.error('[PyodideLoader] loadPyodide failed:', msg, 'errno:', errno);
            throw e;
        }

        // Load micropip for pip support
        try {
            await py.loadPackage('micropip');
        } catch {
            console.warn('[PyodideLoader] micropip load failed (may need network)');
        }

        const loadTime = performance.now() - startTime;
        console.log(`[PyodideLoader] Pyodide loaded in ${loadTime.toFixed(0)}ms`);

        pyodideInstance = py;
        return py;
    })();

    return pyodideLoading;
}

const encoder = new TextEncoder();

/**
 * Run a python3/python command.
 */
function runPython(
    args: string[],
    env: ExecEnv,
    _stdin: InputStream,
    stdout: OutputStream,
    stderr: OutputStream,
): CommandHandle {
    let exitCode: number | undefined;

    const executionPromise = (async () => {
        try {
            const py = await getPyodide();

            // Release all SyncAccessHandle locks held by the shell's OPFS shim
            // so WasmFS's OPFS backend can access the same files.
            closeAllHandles();

            // Set up stdout/stderr capture via Python-level sys.stdout redirect.
            // With WasmFS, Pyodide's device-based stream redirection (setStdout)
            // doesn't work because initializeStreams can't remap device nodes.
            // Instead, we redirect sys.stdout/sys.stderr in Python to call our
            // JS callbacks directly via pyodide.ffi.
            const stdoutWrite = (text: string) => {
                stdout.write(encoder.encode(text));
            };
            const stderrWrite = (text: string) => {
                stderr.write(encoder.encode(text));
            };
            // Register callbacks on the pyodide globals so Python can access them
            (py as any).registerJsModule('_edge_io', {
                stdout_write: stdoutWrite,
                stderr_write: stderrWrite,
            });
            await py.runPythonAsync(`
import sys, io, _edge_io

class _EdgeWriter(io.TextIOBase):
    def __init__(self, write_fn):
        self._write = write_fn
    def write(self, s):
        if s:
            self._write(s)
        return len(s) if s else 0
    def flush(self):
        pass

sys.stdout = _EdgeWriter(_edge_io.stdout_write)
sys.stderr = _EdgeWriter(_edge_io.stderr_write)
`);

            // Set working directory
            const cwd = env.cwd || OPFS_MOUNT_PATH;
            try {
                py.FS.chdir(cwd.startsWith('/') ? cwd : `${OPFS_MOUNT_PATH}/${cwd}`);
            } catch {
                py.FS.chdir(OPFS_MOUNT_PATH);
            }

            // Set environment variables
            if (env.vars.length > 0) {
                const envSetup = env.vars
                    .map(([k, v]) => `import os; os.environ[${JSON.stringify(k)}] = ${JSON.stringify(v)}`)
                    .join('\n');
                await py.runPythonAsync(envSetup);
            }

            // Parse command arguments
            if (args.length === 0) {
                await py.runPythonAsync('import sys; print(f"Python {sys.version}")');
                stdout.write(encoder.encode('Type python3 -c "code" to run Python code\n'));
                exitCode = 0;
                return 0;
            }

            if (args[0] === '-c' && args.length >= 2) {
                const code = args.slice(1).join(' ');
                await py.runPythonAsync(code);
                exitCode = 0;
                return 0;
            }

            if (args[0] === '-m' && args.length >= 2) {
                const moduleName = args[1];
                const moduleArgs = args.slice(2);
                const sysArgv = JSON.stringify([`-m ${moduleName}`, ...moduleArgs]);
                await py.runPythonAsync(`import sys; sys.argv = ${sysArgv}`);
                await py.runPythonAsync(`import runpy; runpy.run_module("${moduleName}", run_name="__main__")`);
                exitCode = 0;
                return 0;
            }

            if (args[0] === '--version' || args[0] === '-V') {
                await py.runPythonAsync('import sys; print(f"Python {sys.version}")');
                exitCode = 0;
                return 0;
            }

            // Script file execution: python3 script.py [args...]
            const scriptPath = args[0];
            const scriptArgs = args.slice(1);

            const resolvedPath = scriptPath.startsWith('/')
                ? scriptPath
                : `${OPFS_MOUNT_PATH}/${env.cwd ? env.cwd + '/' : ''}${scriptPath}`;

            // Read and execute the script via Python (not JS FS.readFile)
            // because WasmFS OPFS reads need JSPI context which is only
            // available inside py.runPythonAsync.
            const sysArgv = JSON.stringify([scriptPath, ...scriptArgs]);
            const escapedPath = JSON.stringify(resolvedPath);
            try {
                await py.runPythonAsync(`
import sys, os
sys.argv = ${sysArgv}
_path = ${escapedPath}
if not os.path.exists(_path):
    raise FileNotFoundError(f"No such file or directory: '{_path}'")
with open(_path) as _f:
    _code = _f.read()
exec(compile(_code, _path, 'exec'))
`);
                exitCode = 0;
                return 0;
            } catch (pyErr: unknown) {
                const msg = String(pyErr);
                if (msg.includes('No such file') || msg.includes('FileNotFoundError') || msg.includes('Errno 44')) {
                    stderr.write(encoder.encode(`python3: can't open file '${scriptPath}': [Errno 2] No such file or directory\n`));
                    exitCode = 2;
                    return 2;
                }
                // Other Python errors — report to stderr
                stderr.write(encoder.encode(msg + '\n'));
                exitCode = 1;
                return 1;
            }
        } catch (err: unknown) {
            const message = err instanceof Error ? err.message : String(err);
            stderr.write(encoder.encode(message + '\n'));
            exitCode = 1;
            return 1;
        }
    })();

    return {
        poll: () => exitCode,
        resolve: () => executionPromise,
    };
}

/**
 * Run a pip command via micropip.
 */
function runPip(
    args: string[],
    _env: ExecEnv,
    _stdin: InputStream,
    stdout: OutputStream,
    stderr: OutputStream,
): CommandHandle {
    let exitCode: number | undefined;

    const executionPromise = (async () => {
        try {
            const py = await getPyodide();

            // Capture Python print() output to the command's streams
            py.setStdout({
                batched: (text: string) => {
                    stdout.write(encoder.encode(text + '\n'));
                },
            });
            py.setStderr({
                batched: (text: string) => {
                    stderr.write(encoder.encode(text + '\n'));
                },
            });

            if (args.length === 0 || args[0] === '--help' || args[0] === '-h') {
                stdout.write(encoder.encode(
                    'Usage: pip install <package> [<package> ...]\n' +
                    '       pip list\n' +
                    '       pip show <package>\n' +
                    '\n' +
                    'Packages are installed via micropip from PyPI.\n'
                ));
                exitCode = 0;
                return 0;
            }

            if (args[0] === 'install' && args.length >= 2) {
                const packages = args.slice(1).filter(a => !a.startsWith('-'));
                stdout.write(encoder.encode(`Installing: ${packages.join(', ')}...\n`));

                for (const pkg of packages) {
                    try {
                        await py.runPythonAsync(
                            `import micropip; await micropip.install(${JSON.stringify(pkg)})`
                        );
                        stdout.write(encoder.encode(`Successfully installed ${pkg}\n`));
                    } catch (err: unknown) {
                        const msg = err instanceof Error ? err.message : String(err);
                        stderr.write(encoder.encode(`ERROR: Could not install ${pkg}: ${msg}\n`));
                        exitCode = 1;
                        return 1;
                    }
                }
                exitCode = 0;
                return 0;
            }

            if (args[0] === 'list') {
                await py.runPythonAsync(`
import micropip
for pkg, version in sorted(micropip.list().items()):
    print(f"{pkg}  {version}")
`);
                exitCode = 0;
                return 0;
            }

            if (args[0] === 'show' && args.length >= 2) {
                const pkg = args[1];
                await py.runPythonAsync(`
import micropip
pkgs = micropip.list()
name = ${JSON.stringify(pkg)}
if name in pkgs:
    print(f"Name: {name}")
    print(f"Version: {pkgs[name]}")
else:
    print(f"WARNING: Package {name} not found")
`);
                exitCode = 0;
                return 0;
            }

            stderr.write(encoder.encode(`pip: unknown command '${args[0]}'\n`));
            exitCode = 1;
            return 1;
        } catch (err: unknown) {
            const message = err instanceof Error ? err.message : String(err);
            stderr.write(encoder.encode(message + '\n'));
            exitCode = 1;
            return 1;
        }
    })();

    return {
        poll: () => exitCode,
        resolve: () => executionPromise,
    };
}

/**
 * Create a CommandModule wrapping Pyodide.
 */
export function createPyodideModule(): CommandModule {
    return {
        spawn(name, args, env, stdin, stdout, stderr) {
            console.log('[PyodideModule] spawn:', name, args);

            if (name === 'pip') {
                return runPip(args, env, stdin, stdout, stderr);
            }

            // python3 or python
            return runPython(args, env, stdin, stdout, stderr);
        },
        listCommands: () => ['python3', 'python', 'pip'],
    };
}
