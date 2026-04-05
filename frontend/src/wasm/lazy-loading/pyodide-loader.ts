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
 * OPFS integration: We mount the OPFS root into Pyodide's Emscripten virtual FS
 * via mountNativeFS(), giving Python seamless access to the same files the shell uses.
 */

import type {
    CommandModule,
    CommandHandle,
    ExecEnv,
    InputStream,
    OutputStream,
} from '@tjfontaine/wasm-loader';

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
        /** Emscripten FS sync: populate=true reads FROM persistent storage, false writes TO it */
        syncfs(populate: boolean, callback: (err: unknown) => void): void;
    };
    mountNativeFS(path: string, handle: FileSystemDirectoryHandle): Promise<{ syncfs(): Promise<void> }>;
    globals: {
        get(name: string): unknown;
    };
}

type LoadPyodideFn = (options: {
    indexURL: string;
    stdout?: (text: string) => void;
    stderr?: (text: string) => void;
}) => Promise<PyodideInterface>;

// Singleton Pyodide instance
let pyodideInstance: PyodideInterface | null = null;
let pyodideLoading: Promise<PyodideInterface> | null = null;
let nativeFsMount: { syncfs(): Promise<void> } | null = null;

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
        // We import the ESM entry point directly to avoid needing pyodide as a
        // frontend dependency (it's a dep of packages/wasm-python instead).
        const pyodideUrl = '/pyodide/pyodide.mjs';
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const pyodideMod: any = await import(/* @vite-ignore */ pyodideUrl);
        const loadPyodide: LoadPyodideFn = pyodideMod.loadPyodide;

        const py = await loadPyodide({
            indexURL: '/pyodide/',
        });

        // Load micropip for pip support
        await py.loadPackage('micropip');

        // Mount OPFS into Pyodide's virtual FS for file access
        try {
            const opfsRoot = await navigator.storage.getDirectory();
            py.FS.mkdir(OPFS_MOUNT_PATH);
            nativeFsMount = await py.mountNativeFS(OPFS_MOUNT_PATH, opfsRoot);
            console.log(`[PyodideLoader] OPFS mounted at ${OPFS_MOUNT_PATH}`);
        } catch (err) {
            console.warn('[PyodideLoader] Could not mount OPFS:', err);
        }

        const loadTime = performance.now() - startTime;
        console.log(`[PyodideLoader] Pyodide loaded in ${loadTime.toFixed(0)}ms`);

        pyodideInstance = py;
        return py;
    })();

    return pyodideLoading;
}

/**
 * Sync OPFS → Emscripten FS (pick up files the shell may have written).
 * Call before running Python so it sees the latest OPFS state.
 */
async function syncFromOpfs(): Promise<void> {
    if (pyodideInstance) {
        await new Promise<void>((resolve, reject) => {
            pyodideInstance!.FS.syncfs(true, (err) => err ? reject(err) : resolve());
        });
    }
}

/**
 * Sync Emscripten FS → OPFS (flush files Python wrote so the shell can see them).
 * Call after Python finishes executing.
 */
async function syncToOpfs(): Promise<void> {
    if (nativeFsMount) {
        await nativeFsMount.syncfs();
    }
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
            await syncFromOpfs();

            // Set up stdout/stderr capture for this invocation
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
                // No args — print version and usage hint
                await py.runPythonAsync('import sys; print(f"Python {sys.version}")');
                stdout.write(encoder.encode('Type python3 -c "code" to run Python code\n'));
                exitCode = 0;
                return 0;
            }

            if (args[0] === '-c' && args.length >= 2) {
                // Inline code execution: python3 -c "print('hello')"
                const code = args.slice(1).join(' ');
                await py.runPythonAsync(code);
                exitCode = 0;
                return 0;
            }

            if (args[0] === '-m' && args.length >= 2) {
                // Module execution: python3 -m module_name
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

            // Set sys.argv
            const sysArgv = JSON.stringify([scriptPath, ...scriptArgs]);
            await py.runPythonAsync(`import sys; sys.argv = ${sysArgv}`);

            // Try to resolve the script path
            const resolvedPath = scriptPath.startsWith('/')
                ? scriptPath
                : `${OPFS_MOUNT_PATH}/${env.cwd ? env.cwd + '/' : ''}${scriptPath}`;

            try {
                const code = py.FS.readFile(resolvedPath, { encoding: 'utf8' });
                await py.runPythonAsync(code);
                exitCode = 0;
                return 0;
            } catch (fsErr) {
                stderr.write(encoder.encode(`python3: can't open file '${scriptPath}': [Errno 2] No such file or directory\n`));
                exitCode = 2;
                return 2;
            }
        } catch (err: unknown) {
            const message = err instanceof Error ? err.message : String(err);
            stderr.write(encoder.encode(message + '\n'));
            exitCode = 1;
            return 1;
        } finally {
            // Flush any files Python wrote back to OPFS so the shell can see them
            await syncToOpfs();
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

            // Fallback: try running via micropip
            stderr.write(encoder.encode(`pip: unknown command '${args[0]}'\n`));
            exitCode = 1;
            return 1;
        } catch (err: unknown) {
            const message = err instanceof Error ? err.message : String(err);
            stderr.write(encoder.encode(message + '\n'));
            exitCode = 1;
            return 1;
        } finally {
            await syncToOpfs();
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
            console.log(`[PyodideModule] spawn: name=${name}, args=`, args);

            if (name === 'pip') {
                return runPip(args, env, stdin, stdout, stderr);
            }

            // python3 or python
            return runPython(args, env, stdin, stdout, stderr);
        },
        listCommands: () => ['python3', 'python', 'pip'],
    };
}
