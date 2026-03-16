/**
 * Direct WASI Preview1 loader for Go WASM modules.
 *
 * Bypasses the WASM Component Model (JCO transpilation) to avoid the
 * stack overflow caused by the wasip1-to-wasip2 adapter's deep call chains.
 * Go's runtime initialization with 568 init functions exhausts the browser's
 * WASM call stack when routed through component model trampolines.
 *
 * Instead, this loader instantiates the raw wasip1 Go binary directly and
 * provides WASI preview1 functions using the existing ghostty-cli-shim
 * infrastructure for stdout/stderr capture.
 */

// Exit error thrown when Go calls proc_exit()
class GoWasmExit extends Error {
    exitError = true;
    code: number;
    constructor(code: number) {
        super(`Go WASM exited with code ${code}`);
        this.code = code;
    }
}

interface GoWasmInstance {
    run(): void;
}

interface GoWasmConfig {
    args: string[];
    env: [string, string][];
    cwd: string;
    stdoutWrite: (data: Uint8Array) => void;
    stderrWrite: (data: Uint8Array) => void;
    /** HTTP bridge request function (raw pointer ABI) */
    httpBridge: {
        request(
            method: string,
            url: string,
            headers: string,
            body: Uint8Array,
        ): number | Promise<number>;
        responseStatus(handle: number): number;
        responseHeaders(handle: number): string;
        responseBodyRead(handle: number, maxBytes: number): Uint8Array;
        responseClose(handle: number): void;
    };
}

/**
 * Load and instantiate a Go wasip1 WASM module directly.
 * Returns an object with a run() method that executes _start().
 */
export async function loadGoWasip1Module(
    wasmUrl: string,
    config: GoWasmConfig,
): Promise<GoWasmInstance> {
    const response = await fetch(wasmUrl);
    const module = await WebAssembly.compileStreaming(response);

    const textDecoder = new TextDecoder();
    const textEncoder = new TextEncoder();

    // Memory reference — updated after instantiation and on memory.grow
    let wasmMemory: WebAssembly.Memory;
    const getMem = (): ArrayBuffer => wasmMemory.buffer;

    // Exports reference — set after instantiation
    let cabiRealloc: (oldPtr: number, oldSize: number, align: number, newSize: number) => number;

    // Encode args and env
    const argBytes = config.args.map(a => textEncoder.encode(a + '\0'));
    const totalArgSize = argBytes.reduce((s, b) => s + b.length, 0);

    const defaultEnv: [string, string][] = [
        ['HOME', config.cwd || '/'],
        ['STRIPE_CLI_TELEMETRY_OPTOUT', 'true'],
    ];
    const allEnv = [...defaultEnv, ...config.env];
    const envPairs = allEnv.map(([k, v]) => textEncoder.encode(k + '=' + v + '\0'));
    const totalEnvSize = envPairs.reduce((s, b) => s + b.length, 0);

    // ========================================================================
    // WASI Preview1 Implementation
    // ========================================================================
    const wasi: Record<string, Function> = {
        args_get(argv_ptr: number, buf_ptr: number): number {
            const view = new DataView(getMem());
            const u8 = new Uint8Array(getMem());
            let offset = buf_ptr;
            for (let i = 0; i < config.args.length; i++) {
                view.setUint32(argv_ptr + i * 4, offset, true);
                u8.set(argBytes[i], offset);
                offset += argBytes[i].length;
            }
            return 0;
        },

        args_sizes_get(argc_ptr: number, size_ptr: number): number {
            const v = new DataView(getMem());
            v.setUint32(argc_ptr, config.args.length, true);
            v.setUint32(size_ptr, totalArgSize, true);
            return 0;
        },

        environ_get(env_ptr: number, buf_ptr: number): number {
            const view = new DataView(getMem());
            const u8 = new Uint8Array(getMem());
            let offset = buf_ptr;
            for (let i = 0; i < envPairs.length; i++) {
                view.setUint32(env_ptr + i * 4, offset, true);
                u8.set(envPairs[i], offset);
                offset += envPairs[i].length;
            }
            return 0;
        },

        environ_sizes_get(count_ptr: number, size_ptr: number): number {
            const v = new DataView(getMem());
            v.setUint32(count_ptr, envPairs.length, true);
            v.setUint32(size_ptr, totalEnvSize, true);
            return 0;
        },

        clock_time_get(_id: number, _precision: bigint, time_ptr: number): number {
            new DataView(getMem()).setBigUint64(
                time_ptr, BigInt(Date.now()) * 1000000n, true,
            );
            return 0;
        },

        fd_write(fd: number, iovs: number, niovs: number, nwritten_ptr: number): number {
            const view = new DataView(getMem());
            let written = 0;
            for (let i = 0; i < niovs; i++) {
                const ptr = view.getUint32(iovs + i * 8, true);
                const len = view.getUint32(iovs + i * 8 + 4, true);
                if (len === 0) continue;
                const bytes = new Uint8Array(getMem(), ptr, len);
                if (fd === 1) {
                    config.stdoutWrite(bytes);
                } else if (fd === 2) {
                    config.stderrWrite(bytes);
                }
                written += len;
            }
            view.setUint32(nwritten_ptr, written, true);
            return 0;
        },

        fd_read(_fd: number, _iovs: number, _niovs: number, nread_ptr: number): number {
            // Return EOF for stdin reads
            new DataView(getMem()).setUint32(nread_ptr, 0, true);
            return 0;
        },

        fd_close(): number { return 0; },
        fd_seek(): number { return 8; }, // EBADF
        fd_sync(): number { return 0; },

        fd_fdstat_get(fd: number, ptr: number): number {
            const v = new DataView(getMem());
            // filetype: character_device (2) for stdio, regular_file (4) otherwise
            v.setUint8(ptr, fd <= 2 ? 2 : 4);
            v.setUint16(ptr + 2, 0, true); // flags
            v.setBigUint64(ptr + 8, 0xFFFFFFFFFFFFFFFFn, true); // rights_base
            v.setBigUint64(ptr + 16, 0xFFFFFFFFFFFFFFFFn, true); // rights_inheriting
            return 0;
        },

        fd_fdstat_set_flags(): number { return 0; },
        fd_filestat_get(): number { return 8; }, // EBADF
        fd_filestat_set_size(): number { return 8; },
        fd_pread(): number { return 8; },
        fd_pwrite(): number { return 8; },
        fd_readdir(): number { return 8; },
        fd_prestat_get(): number { return 8; }, // EBADF — no preopens
        fd_prestat_dir_name(): number { return 8; },

        path_create_directory(): number { return 44; }, // ENOENT
        path_filestat_get(): number { return 44; }, // ENOENT
        path_filestat_set_times(): number { return 44; }, // ENOENT
        path_open(): number { return 44; }, // ENOENT
        path_readlink(): number { return 44; }, // ENOENT
        path_remove_directory(): number { return 44; }, // ENOENT
        path_rename(): number { return 44; }, // ENOENT
        path_symlink(): number { return 44; }, // ENOENT
        path_unlink_file(): number { return 44; }, // ENOENT

        poll_oneoff(
            _in_ptr: number, _out_ptr: number,
            _nsubs: number, nevents_ptr: number,
        ): number {
            new DataView(getMem()).setUint32(nevents_ptr, 0, true);
            return 0;
        },

        proc_exit(code: number): void {
            throw new GoWasmExit(code);
        },

        random_get(buf: number, len: number): number {
            const arr = new Uint8Array(getMem(), buf, len);
            crypto.getRandomValues(arr);
            return 0;
        },

        sched_yield(): number { return 0; },
        sock_accept(): number { return 8; },
        sock_shutdown(): number { return 8; },
    };

    // ========================================================================
    // HTTP Bridge (raw pointer ABI for wasip1)
    // ========================================================================

    /** Read a string from WASM memory */
    function readString(ptr: number, len: number): string {
        return textDecoder.decode(new Uint8Array(getMem(), ptr, len));
    }

    /** Read bytes from WASM memory */
    function readBytes(ptr: number, len: number): Uint8Array {
        return new Uint8Array(getMem(), ptr, len).slice(); // copy to detach from buffer
    }

    /** Allocate WASM memory and write bytes, return ptr */
    function allocAndWrite(data: Uint8Array): number {
        if (data.length === 0) return 0;
        const ptr = cabiRealloc(0, 0, 1, data.length);
        new Uint8Array(getMem(), ptr, data.length).set(data);
        return ptr;
    }

    const bridge: Record<string, Function> = {
        // request(methodPtr, methodLen, urlPtr, urlLen, headersPtr, headersLen, bodyPtr, bodyLen) -> handle
        'request'(
            methodPtr: number, methodLen: number,
            urlPtr: number, urlLen: number,
            headersPtr: number, headersLen: number,
            bodyPtr: number, bodyLen: number,
        ): number | Promise<number> {
            const method = readString(methodPtr, methodLen);
            const url = readString(urlPtr, urlLen);
            const headers = readString(headersPtr, headersLen);
            const body = readBytes(bodyPtr, bodyLen);
            return config.httpBridge.request(method, url, headers, body);
        },

        // response-status(handle) -> u32
        'response-status'(handle: number): number {
            return config.httpBridge.responseStatus(handle);
        },

        // response-headers(handle, retptr) — writes (ptr: u32, len: u32) at retptr
        'response-headers'(handle: number, retptr: number): void {
            const headers = config.httpBridge.responseHeaders(handle);
            const encoded = textEncoder.encode(headers);
            const ptr = allocAndWrite(encoded);
            const view = new DataView(getMem());
            view.setUint32(retptr, ptr, true);
            view.setUint32(retptr + 4, encoded.length, true);
        },

        // response-body-read(handle, maxBytes, retptr) — writes (ptr: u32, len: u32) at retptr
        'response-body-read'(handle: number, maxBytes: number, retptr: number): void {
            const data = config.httpBridge.responseBodyRead(handle, maxBytes);
            const ptr = allocAndWrite(data);
            const view = new DataView(getMem());
            view.setUint32(retptr, ptr, true);
            view.setUint32(retptr + 4, data.length, true);
        },

        // response-close(handle)
        'response-close'(handle: number): void {
            config.httpBridge.responseClose(handle);
        },
    };

    // ========================================================================
    // Instantiate
    // ========================================================================
    const instance = await WebAssembly.instantiate(module, {
        wasi_snapshot_preview1: wasi as WebAssembly.ModuleImports,
        'stripe:bridge/http-bridge@0.1.0': bridge as WebAssembly.ModuleImports,
    });

    wasmMemory = instance.exports.memory as WebAssembly.Memory;
    cabiRealloc = instance.exports.cabi_realloc as (
        oldPtr: number, oldSize: number, align: number, newSize: number,
    ) => number;

    return {
        run(): void {
            (instance.exports._start as () => void)();
        },
    };
}

export { GoWasmExit };
