/**
 * Direct WASI Preview1 loader for Go WASM modules.
 *
 * Bypasses the WASM Component Model (JCO transpilation) to avoid the
 * stack overflow caused by the wasip1-to-wasip2 adapter's deep call chains.
 * Go's runtime initialization with 568 init functions exhausts the browser's
 * WASM call stack when routed through component model trampolines.
 *
 * Instead, this loader instantiates the raw wasip1 Go binary directly and
 * provides full WASI Preview1 filesystem operations backed by OPFS, using
 * the same directory-tree utilities as the rest of the codebase.
 *
 * Async OPFS operations (path_open, etc.) use JSPI (WebAssembly.Suspending)
 * to suspend the WASM stack while JavaScript awaits, matching how the
 * JCO-transpiled modules handle async I/O.
 */

import {
    ensureOpfsRoot,
    normalizePath,
    resolveSymlinks,
    getOpfsFile,
    getOpfsDirectory,
    asyncReadFile,
    asyncWriteFile,
    listDirectory,
    syncHandleCache,
    fileExistsInOpfs,
    directoryExistsInOpfs,
    getFileStats,
    getEntryFromOpfs,
} from '@tjfontaine/wasi-shims/directory-tree.js';
import { hasJSPI } from '@tjfontaine/wasi-shims/execution-mode.js';

// JSPI types not yet in TypeScript's lib
const WA = WebAssembly as typeof WebAssembly & {
    Suspending?: new (fn: Function) => Function;
    promising?: (fn: Function) => Function;
};

// ============================================================================
// WASI Error Codes
// ============================================================================
const ERRNO = {
    SUCCESS:  0,
    BADF:     8,
    EXIST:   20,
    INVAL:   28,
    ISDIR:   31,
    NOENT:   44,
    NOSYS:   52,
    NOTDIR:  54,
    NOTEMPTY: 55,
    NXIO:    60,
} as const;

// WASI file types
const FILETYPE = {
    UNKNOWN:          0,
    BLOCK_DEVICE:     1,
    CHARACTER_DEVICE: 2,
    DIRECTORY:        3,
    REGULAR_FILE:     4,
    SYMLINK:          7,
} as const;

// WASI open flags (oflags)
const OFLAGS = {
    CREAT:    1,
    DIRECTORY: 2,
    EXCL:     4,
    TRUNC:    8,
} as const;

// WASI whence
const WHENCE = {
    SET: 0,
    CUR: 1,
    END: 2,
} as const;

// ============================================================================
// GoWasmExit
// ============================================================================
class GoWasmExit extends Error {
    exitError = true;
    code: number;
    constructor(code: number) {
        super(`Go WASM exited with code ${code}`);
        this.code = code;
    }
}

// ============================================================================
// File Descriptor Table
// ============================================================================

interface FdEntry {
    type: 'stdio' | 'file' | 'dir' | 'preopen';
    /** Normalized path (empty string for root) */
    path: string;
    /** Display path for preopens */
    preopenPath?: string;
    /** Seek offset for files */
    offset: number;
    /** SyncAccessHandle if available */
    syncHandle?: FileSystemSyncAccessHandle;
    /** Cached file size */
    size?: number;
    /** Directory entries for readdir iteration */
    dirEntries?: Array<{ name: string; type: number; size: number; mtime: number }>;
    /** readdir cookie tracking */
    dirCookie?: number;
}

interface GoWasmInstance {
    run(): void | Promise<void>;
}

interface GoWasmConfig {
    args: string[];
    env: [string, string][];
    cwd: string;
    stdoutWrite: (data: Uint8Array) => void;
    stderrWrite: (data: Uint8Array) => void;
    httpBridge: {
        request(method: string, url: string, headers: string, body: Uint8Array): number | Promise<number>;
        responseStatus(handle: number): number;
        responseHeaders(handle: number): string;
        responseBodyRead(handle: number, maxBytes: number): Uint8Array;
        responseClose(handle: number): void;
    };
    wsBridge?: {
        connect(url: string): number | Promise<number>;
        read(handle: number, maxBytes: number): Uint8Array | Promise<Uint8Array>;
        write(handle: number, data: Uint8Array): number;
        close(handle: number): void;
    };
    openUrl?: (url: string) => void | Promise<void>;
}

/**
 * Load and instantiate a Go wasip1 WASM module directly.
 * Returns an object with a run() method that executes _start().
 */
export async function loadGoWasip1Module(
    wasmUrl: string,
    config: GoWasmConfig,
): Promise<GoWasmInstance> {
    // Ensure OPFS is available
    await ensureOpfsRoot();

    const response = await fetch(wasmUrl);
    const module = await WebAssembly.compileStreaming(response);

    const textDecoder = new TextDecoder();
    const textEncoder = new TextEncoder();

    // Memory reference — updated after instantiation
    let wasmMemory: WebAssembly.Memory;
    const getMem = (): ArrayBuffer => wasmMemory.buffer;

    // Exports reference — set after instantiation
    let cabiRealloc: (oldPtr: number, oldSize: number, align: number, newSize: number) => number;

    // ========================================================================
    // File Descriptor Table
    // ========================================================================
    const fdTable = new Map<number, FdEntry>();
    let nextFd = 4; // 0=stdin, 1=stdout, 2=stderr, 3=preopen root

    // Initialize stdio and preopen
    fdTable.set(0, { type: 'stdio', path: '', offset: 0 });
    fdTable.set(1, { type: 'stdio', path: '', offset: 0 });
    fdTable.set(2, { type: 'stdio', path: '', offset: 0 });
    fdTable.set(3, { type: 'preopen', path: '', preopenPath: '/', offset: 0 });

    function allocFd(entry: FdEntry): number {
        const fd = nextFd++;
        fdTable.set(fd, entry);
        return fd;
    }

    /** Resolve a path relative to a directory fd */
    function resolvePath(dirFd: number, subpath: string): string {
        const dirEntry = fdTable.get(dirFd);
        if (!dirEntry) return subpath;
        const base = dirEntry.path;
        if (!base) return subpath;
        return base + '/' + subpath;
    }

    /** Read a nul-terminated or length-bounded string from WASM memory */
    function readString(ptr: number, len: number): string {
        return textDecoder.decode(new Uint8Array(getMem(), ptr, len));
    }

    /** Read bytes from WASM memory (copy to detach from buffer) */
    function readBytes(ptr: number, len: number): Uint8Array {
        return new Uint8Array(getMem(), ptr, len).slice();
    }

    /** Allocate WASM memory and write bytes, return ptr */
    function allocAndWrite(data: Uint8Array): number {
        if (data.length === 0) return 0;
        const ptr = cabiRealloc(0, 0, 1, data.length);
        new Uint8Array(getMem(), ptr, data.length).set(data);
        return ptr;
    }

    // ========================================================================
    // Encode args and env
    // ========================================================================
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
    // Async filesystem helpers (wrapped with JSPI when available)
    // ========================================================================

    async function asyncPathOpen(
        dirfd: number, _dirflags: number,
        pathPtr: number, pathLen: number,
        oflags: number, _rightsBase: bigint, _rightsInheriting: bigint,
        _fdflags: number, fdPtr: number,
    ): Promise<number> {
        const subpath = readString(pathPtr, pathLen);
        const rawPath = resolvePath(dirfd, subpath);
        const resolvedPath = resolveSymlinks(rawPath);
        const normalizedPath = normalizePath(resolvedPath);

        const wantCreate = (oflags & OFLAGS.CREAT) !== 0;
        const wantDirectory = (oflags & OFLAGS.DIRECTORY) !== 0;
        const wantExcl = (oflags & OFLAGS.EXCL) !== 0;
        const wantTrunc = (oflags & OFLAGS.TRUNC) !== 0;

        try {
            const entry = await getEntryFromOpfs(normalizedPath);

            if (entry) {
                if (wantExcl && wantCreate) return ERRNO.EXIST;

                if (entry.dir !== undefined) {
                    const fd = allocFd({ type: 'dir', path: normalizedPath, offset: 0 });
                    new DataView(getMem()).setUint32(fdPtr, fd, true);
                    return ERRNO.SUCCESS;
                }

                // Regular file
                if (wantDirectory) return ERRNO.NOTDIR;

                let syncHandle: FileSystemSyncAccessHandle | undefined;
                let size = entry.size || 0;

                // Try to get a SyncAccessHandle for fast I/O
                try {
                    const existingHandle = syncHandleCache.get(normalizedPath);
                    if (existingHandle) {
                        syncHandle = existingHandle;
                    } else {
                        const fileHandle = await getOpfsFile(normalizedPath, false);
                        syncHandle = await fileHandle.createSyncAccessHandle();
                        syncHandleCache.set(normalizedPath, syncHandle);
                    }
                    size = syncHandle.getSize();
                } catch {
                    // Fall back to async reads/writes
                }

                if (wantTrunc && syncHandle) {
                    syncHandle.truncate(0);
                    syncHandle.flush();
                    size = 0;
                }

                const fd = allocFd({
                    type: 'file', path: normalizedPath, offset: 0,
                    syncHandle, size,
                });
                new DataView(getMem()).setUint32(fdPtr, fd, true);
                return ERRNO.SUCCESS;
            }

            // Entry doesn't exist
            if (!wantCreate) return ERRNO.NOENT;

            if (wantDirectory) {
                const parts = normalizedPath.split('/').filter(p => p);
                await getOpfsDirectory(parts, true);
                const fd = allocFd({ type: 'dir', path: normalizedPath, offset: 0 });
                new DataView(getMem()).setUint32(fdPtr, fd, true);
                return ERRNO.SUCCESS;
            }

            // Create file
            const fileHandle = await getOpfsFile(normalizedPath, true);
            let syncHandle: FileSystemSyncAccessHandle | undefined;
            try {
                syncHandle = await fileHandle.createSyncAccessHandle();
                syncHandleCache.set(normalizedPath, syncHandle);
            } catch {
                // Fall back to async
            }

            const fd = allocFd({
                type: 'file', path: normalizedPath, offset: 0,
                syncHandle, size: 0,
            });
            new DataView(getMem()).setUint32(fdPtr, fd, true);
            return ERRNO.SUCCESS;
        } catch (e) {
            console.error('[go-wasip1] path_open error:', normalizedPath, e);
            return ERRNO.NOENT;
        }
    }

    async function asyncPathCreateDirectory(
        dirfd: number, pathPtr: number, pathLen: number,
    ): Promise<number> {
        const subpath = readString(pathPtr, pathLen);
        const rawPath = resolvePath(dirfd, subpath);
        const normalizedPath = normalizePath(resolveSymlinks(rawPath));

        try {
            const existing = await directoryExistsInOpfs(normalizedPath);
            if (existing) return ERRNO.EXIST;
            const parts = normalizedPath.split('/').filter(p => p);
            await getOpfsDirectory(parts, true);
            return ERRNO.SUCCESS;
        } catch (e) {
            console.error('[go-wasip1] path_create_directory error:', normalizedPath, e);
            return ERRNO.NOENT;
        }
    }

    async function asyncPathFilestatGet(
        dirfd: number, _flags: number, pathPtr: number, pathLen: number, retPtr: number,
    ): Promise<number> {
        const subpath = readString(pathPtr, pathLen);
        const rawPath = resolvePath(dirfd, subpath);
        const normalizedPath = normalizePath(resolveSymlinks(rawPath));
        const view = new DataView(getMem());

        try {
            const entry = await getEntryFromOpfs(normalizedPath);
            if (!entry) return ERRNO.NOENT;

            if (entry.dir !== undefined) {
                writeFilestat(view, retPtr, FILETYPE.DIRECTORY, 0, Date.now());
            } else {
                writeFilestat(view, retPtr, FILETYPE.REGULAR_FILE, entry.size || 0, entry.mtime || Date.now());
            }
            return ERRNO.SUCCESS;
        } catch {
            return ERRNO.NOENT;
        }
    }

    async function asyncPathUnlinkFile(
        dirfd: number, pathPtr: number, pathLen: number,
    ): Promise<number> {
        const subpath = readString(pathPtr, pathLen);
        const rawPath = resolvePath(dirfd, subpath);
        const normalizedPath = normalizePath(resolveSymlinks(rawPath));

        try {
            // Close any sync handle first
            const handle = syncHandleCache.get(normalizedPath);
            if (handle) {
                try { handle.close(); } catch { /* ignore */ }
                syncHandleCache.delete(normalizedPath);
            }

            // Navigate to parent directory and remove the file
            const parts = normalizedPath.split('/').filter(p => p);
            if (parts.length === 0) return ERRNO.NOENT;
            const fileName = parts.pop()!;
            const parentParts = parts;
            const parentDir = parentParts.length > 0
                ? await getOpfsDirectory(parentParts, false)
                : await ensureOpfsRoot();
            await parentDir.removeEntry(fileName);
            return ERRNO.SUCCESS;
        } catch {
            return ERRNO.NOENT;
        }
    }

    async function asyncPathRemoveDirectory(
        dirfd: number, pathPtr: number, pathLen: number,
    ): Promise<number> {
        const subpath = readString(pathPtr, pathLen);
        const rawPath = resolvePath(dirfd, subpath);
        const normalizedPath = normalizePath(resolveSymlinks(rawPath));

        try {
            const parts = normalizedPath.split('/').filter(p => p);
            if (parts.length === 0) return ERRNO.NOENT;
            const dirName = parts.pop()!;
            const parentParts = parts;
            const parentDir = parentParts.length > 0
                ? await getOpfsDirectory(parentParts, false)
                : await ensureOpfsRoot();
            await parentDir.removeEntry(dirName, { recursive: true });
            return ERRNO.SUCCESS;
        } catch {
            return ERRNO.NOENT;
        }
    }

    async function asyncPathRename(
        oldDirfd: number, oldPathPtr: number, oldPathLen: number,
        newDirfd: number, newPathPtr: number, newPathLen: number,
    ): Promise<number> {
        const oldSubpath = readString(oldPathPtr, oldPathLen);
        const newSubpath = readString(newPathPtr, newPathLen);
        const oldPath = normalizePath(resolveSymlinks(resolvePath(oldDirfd, oldSubpath)));
        const newPath = normalizePath(resolveSymlinks(resolvePath(newDirfd, newSubpath)));

        try {
            // OPFS doesn't have rename — copy and delete
            const data = await asyncReadFile(oldPath);
            await asyncWriteFile(newPath, data);

            // Close old handle and remove old file
            const handle = syncHandleCache.get(oldPath);
            if (handle) {
                try { handle.close(); } catch { /* ignore */ }
                syncHandleCache.delete(oldPath);
            }
            const oldParts = oldPath.split('/').filter(p => p);
            const fileName = oldParts.pop()!;
            const parentDir = oldParts.length > 0
                ? await getOpfsDirectory(oldParts, false)
                : await ensureOpfsRoot();
            await parentDir.removeEntry(fileName);

            return ERRNO.SUCCESS;
        } catch {
            return ERRNO.NOENT;
        }
    }

    async function asyncFdReaddir(
        fd: number, bufPtr: number, bufLen: number, cookie: bigint, retPtr: number,
    ): Promise<number> {
        const entry = fdTable.get(fd);
        if (!entry || (entry.type !== 'dir' && entry.type !== 'preopen')) return ERRNO.BADF;

        const view = new DataView(getMem());
        const u8 = new Uint8Array(getMem());
        const startCookie = Number(cookie);

        try {
            // Lazily load directory entries
            if (!entry.dirEntries || entry.dirCookie !== startCookie) {
                const entries = await listDirectory(entry.path);
                entry.dirEntries = entries.map(e => ({
                    name: e.name,
                    type: e.isDirectory ? FILETYPE.DIRECTORY : FILETYPE.REGULAR_FILE,
                    size: e.size || 0,
                    mtime: e.mtime || 0,
                }));
            }

            let offset = 0;
            for (let i = startCookie; i < entry.dirEntries.length; i++) {
                const de = entry.dirEntries[i];
                const nameBytes = textEncoder.encode(de.name);
                // dirent: d_next(8) + d_ino(8) + d_namlen(4) + d_type(1) = 24 bytes + name
                const entrySize = 24 + nameBytes.length;

                if (offset + entrySize > bufLen) break;

                const base = bufPtr + offset;
                view.setBigUint64(base, BigInt(i + 1), true); // d_next (cookie for next entry)
                view.setBigUint64(base + 8, 0n, true);        // d_ino
                view.setUint32(base + 16, nameBytes.length, true); // d_namlen
                view.setUint8(base + 20, de.type);                 // d_type
                u8.set(nameBytes, base + 24);
                offset += entrySize;
            }

            view.setUint32(retPtr, offset, true);
            return ERRNO.SUCCESS;
        } catch {
            return ERRNO.BADF;
        }
    }

    async function asyncFdFilestatGet(fd: number, retPtr: number): Promise<number> {
        const entry = fdTable.get(fd);
        if (!entry) return ERRNO.BADF;

        const view = new DataView(getMem());

        if (entry.type === 'stdio') {
            writeFilestat(view, retPtr, FILETYPE.CHARACTER_DEVICE, 0, 0);
            return ERRNO.SUCCESS;
        }

        if (entry.type === 'dir' || entry.type === 'preopen') {
            writeFilestat(view, retPtr, FILETYPE.DIRECTORY, 0, Date.now());
            return ERRNO.SUCCESS;
        }

        // File: get real size
        if (entry.syncHandle) {
            const size = entry.syncHandle.getSize();
            writeFilestat(view, retPtr, FILETYPE.REGULAR_FILE, size, Date.now());
            return ERRNO.SUCCESS;
        }

        try {
            const stats = await getFileStats(entry.path);
            if (stats) {
                writeFilestat(view, retPtr, FILETYPE.REGULAR_FILE, stats.size, stats.mtime);
            } else {
                writeFilestat(view, retPtr, FILETYPE.REGULAR_FILE, entry.size || 0, Date.now());
            }
            return ERRNO.SUCCESS;
        } catch {
            return ERRNO.BADF;
        }
    }

    async function asyncFdRead(
        fd: number, iovs: number, niovs: number, nreadPtr: number,
    ): Promise<number> {
        if (fd <= 2) {
            // stdin: return EOF
            new DataView(getMem()).setUint32(nreadPtr, 0, true);
            return ERRNO.SUCCESS;
        }

        const entry = fdTable.get(fd);
        if (!entry || entry.type !== 'file') return ERRNO.BADF;

        const view = new DataView(getMem());
        let totalRead = 0;

        if (entry.syncHandle) {
            for (let i = 0; i < niovs; i++) {
                const ptr = view.getUint32(iovs + i * 8, true);
                const len = view.getUint32(iovs + i * 8 + 4, true);
                if (len === 0) continue;

                const buf = new Uint8Array(getMem(), ptr, len);
                const bytesRead = entry.syncHandle.read(buf, { at: entry.offset });
                entry.offset += bytesRead;
                totalRead += bytesRead;
                if (bytesRead < len) break; // EOF
            }
        } else {
            // Async fallback
            try {
                const data = await asyncReadFile(entry.path);
                for (let i = 0; i < niovs; i++) {
                    const ptr = view.getUint32(iovs + i * 8, true);
                    const len = view.getUint32(iovs + i * 8 + 4, true);
                    if (len === 0) continue;

                    const available = Math.min(len, data.length - entry.offset);
                    if (available <= 0) break;

                    new Uint8Array(getMem(), ptr, available).set(
                        data.subarray(entry.offset, entry.offset + available),
                    );
                    entry.offset += available;
                    totalRead += available;
                    if (available < len) break;
                }
            } catch {
                return ERRNO.BADF;
            }
        }

        view.setUint32(nreadPtr, totalRead, true);
        return ERRNO.SUCCESS;
    }

    async function asyncFdWrite(
        fd: number, iovs: number, niovs: number, nwrittenPtr: number,
    ): Promise<number> {
        const view = new DataView(getMem());

        // stdio: delegate to config callbacks
        if (fd === 1 || fd === 2) {
            let written = 0;
            for (let i = 0; i < niovs; i++) {
                const ptr = view.getUint32(iovs + i * 8, true);
                const len = view.getUint32(iovs + i * 8 + 4, true);
                if (len === 0) continue;
                const bytes = new Uint8Array(getMem(), ptr, len);
                if (fd === 1) config.stdoutWrite(bytes);
                else config.stderrWrite(bytes);
                written += len;
            }
            view.setUint32(nwrittenPtr, written, true);
            return ERRNO.SUCCESS;
        }

        const entry = fdTable.get(fd);
        if (!entry || entry.type !== 'file') return ERRNO.BADF;

        let totalWritten = 0;

        if (entry.syncHandle) {
            for (let i = 0; i < niovs; i++) {
                const ptr = view.getUint32(iovs + i * 8, true);
                const len = view.getUint32(iovs + i * 8 + 4, true);
                if (len === 0) continue;

                const buf = new Uint8Array(getMem(), ptr, len);
                const bytesWritten = entry.syncHandle.write(buf, { at: entry.offset });
                entry.offset += bytesWritten;
                totalWritten += bytesWritten;
            }
            entry.syncHandle.flush();
            entry.size = entry.syncHandle.getSize();
        } else {
            // Async fallback: accumulate all iovecs and write at once
            const chunks: Uint8Array[] = [];
            for (let i = 0; i < niovs; i++) {
                const ptr = view.getUint32(iovs + i * 8, true);
                const len = view.getUint32(iovs + i * 8 + 4, true);
                if (len === 0) continue;
                chunks.push(readBytes(ptr, len));
                totalWritten += len;
            }

            if (chunks.length > 0) {
                const totalLen = chunks.reduce((s, c) => s + c.length, 0);
                const combined = new Uint8Array(totalLen);
                let off = 0;
                for (const chunk of chunks) {
                    combined.set(chunk, off);
                    off += chunk.length;
                }
                try {
                    await asyncWriteFile(entry.path, combined);
                    entry.offset += totalWritten;
                    entry.size = (entry.size || 0) + totalWritten;
                } catch {
                    return ERRNO.BADF;
                }
            }
        }

        view.setUint32(nwrittenPtr, totalWritten, true);
        return ERRNO.SUCCESS;
    }

    /** Write a filestat structure at the given offset */
    function writeFilestat(
        view: DataView, ptr: number,
        filetype: number, size: number, mtimeNs: number,
    ): void {
        // filestat: dev(8) + ino(8) + filetype(1) + nlink(8) + size(8) + atim(8) + mtim(8) + ctim(8) = 64 bytes
        view.setBigUint64(ptr, 0n, true);       // dev
        view.setBigUint64(ptr + 8, 0n, true);   // ino
        view.setUint8(ptr + 16, filetype);       // filetype
        view.setBigUint64(ptr + 24, 1n, true);  // nlink
        view.setBigUint64(ptr + 32, BigInt(size), true);  // size
        const ns = BigInt(mtimeNs) * 1000000n;
        view.setBigUint64(ptr + 40, ns, true);   // atim
        view.setBigUint64(ptr + 48, ns, true);   // mtim
        view.setBigUint64(ptr + 56, ns, true);   // ctim
    }

    // ========================================================================
    // WASI Preview1 Implementation
    // ========================================================================

    // Preopen path as bytes (for fd_prestat_dir_name)
    const preopenPathBytes = textEncoder.encode('/');

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
            return ERRNO.SUCCESS;
        },

        args_sizes_get(argc_ptr: number, size_ptr: number): number {
            const v = new DataView(getMem());
            v.setUint32(argc_ptr, config.args.length, true);
            v.setUint32(size_ptr, totalArgSize, true);
            return ERRNO.SUCCESS;
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
            return ERRNO.SUCCESS;
        },

        environ_sizes_get(count_ptr: number, size_ptr: number): number {
            const v = new DataView(getMem());
            v.setUint32(count_ptr, envPairs.length, true);
            v.setUint32(size_ptr, totalEnvSize, true);
            return ERRNO.SUCCESS;
        },

        clock_time_get(_id: number, _precision: bigint, time_ptr: number): number {
            new DataView(getMem()).setBigUint64(
                time_ptr, BigInt(Date.now()) * 1000000n, true,
            );
            return ERRNO.SUCCESS;
        },

        // fd_prestat_get(fd, retptr) -> errno
        // retptr layout: u8 tag (0 = dir) + 3 pad + u32 name_len
        fd_prestat_get(fd: number, retPtr: number): number {
            const entry = fdTable.get(fd);
            if (!entry || entry.type !== 'preopen') return ERRNO.BADF;
            const view = new DataView(getMem());
            view.setUint8(retPtr, 0); // tag: directory
            view.setUint32(retPtr + 4, preopenPathBytes.length, true);
            return ERRNO.SUCCESS;
        },

        // fd_prestat_dir_name(fd, path_ptr, path_len) -> errno
        fd_prestat_dir_name(fd: number, pathPtr: number, pathLen: number): number {
            const entry = fdTable.get(fd);
            if (!entry || entry.type !== 'preopen') return ERRNO.BADF;
            const u8 = new Uint8Array(getMem());
            const bytes = preopenPathBytes.subarray(0, pathLen);
            u8.set(bytes, pathPtr);
            return ERRNO.SUCCESS;
        },

        fd_fdstat_get(fd: number, ptr: number): number {
            const entry = fdTable.get(fd);
            if (!entry) return ERRNO.BADF;

            const v = new DataView(getMem());
            let filetype: number;
            switch (entry.type) {
                case 'stdio': filetype = FILETYPE.CHARACTER_DEVICE; break;
                case 'dir':
                case 'preopen': filetype = FILETYPE.DIRECTORY; break;
                case 'file': filetype = FILETYPE.REGULAR_FILE; break;
                default: filetype = FILETYPE.UNKNOWN;
            }
            v.setUint8(ptr, filetype);
            v.setUint16(ptr + 2, 0, true); // flags
            v.setBigUint64(ptr + 8, 0xFFFFFFFFFFFFFFFFn, true);  // rights_base
            v.setBigUint64(ptr + 16, 0xFFFFFFFFFFFFFFFFn, true); // rights_inheriting
            return ERRNO.SUCCESS;
        },

        fd_fdstat_set_flags(): number { return ERRNO.SUCCESS; },

        // fd_close(fd) -> errno
        fd_close(fd: number): number {
            const entry = fdTable.get(fd);
            if (!entry) return ERRNO.BADF;
            if (fd <= 2) return ERRNO.SUCCESS; // Don't close stdio

            // Release sync handle
            if (entry.syncHandle) {
                try { entry.syncHandle.close(); } catch { /* ignore */ }
                syncHandleCache.delete(entry.path);
            }

            fdTable.delete(fd);
            return ERRNO.SUCCESS;
        },

        // fd_seek(fd, offset, whence, newoffset_ptr) -> errno
        fd_seek(fd: number, offset: bigint, whence: number, newoffsetPtr: number): number {
            const entry = fdTable.get(fd);
            if (!entry || entry.type !== 'file') return ERRNO.BADF;

            const offsetNum = Number(offset);
            let newOffset: number;

            switch (whence) {
                case WHENCE.SET:
                    newOffset = offsetNum;
                    break;
                case WHENCE.CUR:
                    newOffset = entry.offset + offsetNum;
                    break;
                case WHENCE.END: {
                    const size = entry.syncHandle
                        ? entry.syncHandle.getSize()
                        : (entry.size || 0);
                    newOffset = size + offsetNum;
                    break;
                }
                default:
                    return ERRNO.INVAL;
            }

            entry.offset = Math.max(0, newOffset);
            new DataView(getMem()).setBigUint64(newoffsetPtr, BigInt(entry.offset), true);
            return ERRNO.SUCCESS;
        },

        fd_sync(fd: number): number {
            const entry = fdTable.get(fd);
            if (entry?.syncHandle) {
                entry.syncHandle.flush();
            }
            return ERRNO.SUCCESS;
        },

        fd_filestat_set_size(fd: number, size: bigint): number {
            const entry = fdTable.get(fd);
            if (!entry || entry.type !== 'file') return ERRNO.BADF;

            if (entry.syncHandle) {
                entry.syncHandle.truncate(Number(size));
                entry.syncHandle.flush();
                entry.size = Number(size);
            }
            return ERRNO.SUCCESS;
        },

        fd_pread(): number { return ERRNO.NOSYS; },
        fd_pwrite(): number { return ERRNO.NOSYS; },

        path_filestat_set_times(): number { return ERRNO.SUCCESS; },
        path_readlink(): number { return ERRNO.NOENT; },
        path_symlink(): number { return ERRNO.NOSYS; },

        poll_oneoff(
            _in_ptr: number, _out_ptr: number,
            _nsubs: number, nevents_ptr: number,
        ): number {
            new DataView(getMem()).setUint32(nevents_ptr, 0, true);
            return ERRNO.SUCCESS;
        },

        proc_exit(code: number): void {
            // Close all file descriptors before exit
            for (const [fd, entry] of fdTable) {
                if (fd > 2 && entry.syncHandle) {
                    try { entry.syncHandle.close(); } catch { /* ignore */ }
                    syncHandleCache.delete(entry.path);
                }
            }
            throw new GoWasmExit(code);
        },

        random_get(buf: number, len: number): number {
            const arr = new Uint8Array(getMem(), buf, len);
            crypto.getRandomValues(arr);
            return ERRNO.SUCCESS;
        },

        sched_yield(): number { return ERRNO.SUCCESS; },
        sock_accept(): number { return ERRNO.NOSYS; },
        sock_shutdown(): number { return ERRNO.NOSYS; },
    };

    // ========================================================================
    // Wire up async WASI functions — JSPI-wrapped or direct
    // ========================================================================

    // These WASI functions need async OPFS access. With JSPI, they're wrapped
    // with WebAssembly.Suspending so the WASM stack suspends on the Promise.
    // Without JSPI, they run synchronously (returning ENOENT for operations
    // that can't be done synchronously).
    const asyncFunctions: Record<string, Function> = {
        path_open: asyncPathOpen,
        path_create_directory: asyncPathCreateDirectory,
        path_filestat_get: asyncPathFilestatGet,
        path_unlink_file: asyncPathUnlinkFile,
        path_remove_directory: asyncPathRemoveDirectory,
        path_rename: asyncPathRename,
        fd_readdir: asyncFdReaddir,
        fd_filestat_get: asyncFdFilestatGet,
        fd_read: asyncFdRead,
        fd_write: asyncFdWrite,
    };

    if (hasJSPI && WA.Suspending) {
        // JSPI mode: wrap async functions so WASM suspends on their Promises
        for (const [name, fn] of Object.entries(asyncFunctions)) {
            wasi[name] = new WA.Suspending(fn);
        }
    } else {
        // Non-JSPI mode: use the async functions directly.
        // For path operations that truly need async, fall back to ENOENT stubs.
        // fd_read and fd_write for stdio are synchronous and work fine.
        // File I/O through SyncAccessHandle is also synchronous.
        for (const [name, fn] of Object.entries(asyncFunctions)) {
            wasi[name] = fn;
        }
    }

    // ========================================================================
    // HTTP Bridge (raw pointer ABI for wasip1)
    // ========================================================================
    const requestFn = function (
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
    };

    // In JSPI mode, wrap request with WebAssembly.Suspending so the WASM
    // stack properly suspends while the fetch Promise resolves. Without this,
    // the Promise object gets coerced to 0 and subsequent response reads fail
    // with "invalid handle 0".
    const wrappedRequest = (hasJSPI && WA.Suspending)
        ? new WA.Suspending(requestFn)
        : requestFn;

    const bridge: Record<string, Function> = {
        'request': wrappedRequest,

        'response-status'(handle: number): number {
            return config.httpBridge.responseStatus(handle);
        },

        'response-headers'(handle: number, retptr: number): void {
            const headers = config.httpBridge.responseHeaders(handle);
            const encoded = textEncoder.encode(headers);
            const ptr = allocAndWrite(encoded);
            const view = new DataView(getMem());
            view.setUint32(retptr, ptr, true);
            view.setUint32(retptr + 4, encoded.length, true);
        },

        'response-body-read'(handle: number, maxBytes: number, retptr: number): void {
            const data = config.httpBridge.responseBodyRead(handle, maxBytes);
            const ptr = allocAndWrite(data);
            const view = new DataView(getMem());
            view.setUint32(retptr, ptr, true);
            view.setUint32(retptr + 4, data.length, true);
        },

        'response-close'(handle: number): void {
            config.httpBridge.responseClose(handle);
        },
    };

    // ========================================================================
    // WebSocket Bridge (raw pointer ABI for wasip1)
    // ========================================================================
    // Only provided when config.wsBridge is set (stripe-module needs it for
    // `stripe listen`; git-module does not use WebSockets).
    const wsBridgeImports: Record<string, Function> = {};
    if (config.wsBridge) {
        const ws = config.wsBridge;

        // connect: func(url: string) -> u32
        // canonical ABI: (url_ptr, url_len) -> handle
        const connectFn = function (urlPtr: number, urlLen: number): number | Promise<number> {
            const url = readString(urlPtr, urlLen);
            return ws.connect(url);
        };
        wsBridgeImports['connect'] = (hasJSPI && WA.Suspending)
            ? new WA.Suspending(connectFn)
            : connectFn;

        // read: func(handle: u32, max-bytes: u32) -> list<u8>
        // canonical ABI: (handle, max_bytes, retptr) -> void
        const readFn = async function (handle: number, maxBytes: number, retptr: number): Promise<void> {
            const data = await ws.read(handle, maxBytes);
            const ptr = allocAndWrite(data);
            const view = new DataView(getMem());
            view.setUint32(retptr, ptr, true);
            view.setUint32(retptr + 4, data.length, true);
        };
        wsBridgeImports['read'] = (hasJSPI && WA.Suspending)
            ? new WA.Suspending(readFn)
            : function (handle: number, maxBytes: number, retptr: number): void {
                // Sync fallback: non-blocking read returns empty if nothing queued
                const data = ws.read(handle, maxBytes) as Uint8Array;
                const ptr = allocAndWrite(data);
                const view = new DataView(getMem());
                view.setUint32(retptr, ptr, true);
                view.setUint32(retptr + 4, data.length, true);
            };

        // write: func(handle: u32, data: list<u8>) -> u32
        // canonical ABI: (handle, data_ptr, data_len) -> written
        wsBridgeImports['write'] = function (handle: number, dataPtr: number, dataLen: number): number {
            const data = readBytes(dataPtr, dataLen);
            return ws.write(handle, data);
        };

        // close: func(handle: u32)
        wsBridgeImports['close'] = function (handle: number): void {
            ws.close(handle);
        };
    }

    // ========================================================================
    // Instantiate
    // ========================================================================
    const imports: Record<string, WebAssembly.ModuleImports> = {
        wasi_snapshot_preview1: wasi as WebAssembly.ModuleImports,
        'stripe:bridge/http-bridge@0.1.0': bridge as WebAssembly.ModuleImports,
        'git:bridge/http-bridge@0.1.0': bridge as WebAssembly.ModuleImports,
    };
    if (config.wsBridge) {
        imports['stripe:bridge/ws-bridge@0.1.0'] = wsBridgeImports as WebAssembly.ModuleImports;
    }

    // ========================================================================
    // Browser Actions (open-url for stripe login / community links)
    // ========================================================================
    if (config.openUrl) {
        const openUrlFn = function (urlPtr: number, urlLen: number): void | Promise<void> {
            const url = readString(urlPtr, urlLen);
            return config.openUrl!(url);
        };
        imports['host:browser/actions@0.1.0'] = {
            'open-url': (hasJSPI && WA.Suspending)
                ? new WA.Suspending(openUrlFn)
                : openUrlFn,
        };
    }

    const instance = await WebAssembly.instantiate(module, imports);

    wasmMemory = instance.exports.memory as WebAssembly.Memory;
    cabiRealloc = instance.exports.cabi_realloc as (
        oldPtr: number, oldSize: number, align: number, newSize: number,
    ) => number;

    // Wrap _start with WebAssembly.promising if JSPI is available,
    // so it returns a Promise that resolves when WASM finishes (or suspends).
    const rawStart = instance.exports._start as () => void;
    const promisedStart = (hasJSPI && WA.promising)
        ? WA.promising(rawStart) as () => Promise<void>
        : null;

    return {
        run(): void | Promise<void> {
            if (promisedStart) {
                return promisedStart();
            }
            return rawStart();
        },
    };
}

export { GoWasmExit };
