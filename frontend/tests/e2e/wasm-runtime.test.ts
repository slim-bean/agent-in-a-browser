/**
 * WASM Runtime E2E Tests
 * 
 * Tests the actual WASM component running in a real browser environment.
 * Uses Playwright to automate browser testing and interact with the sandbox worker.
 * 
 * NOTE: The browser uses OPFS (async filesystem), so sync fs operations are not available.
 * These tests verify what actually works in the browser environment.
 */

// Use webkit-persistent-fixture for OPFS support in Safari/WebKit
import { test, expect } from './webkit-persistent-fixture';
import type { Page } from '@playwright/test';

// Helper to execute commands through the sandbox worker
async function shellEval(page: Page, command: string): Promise<{ output: string; success: boolean; error?: string }> {
    const result = await page.evaluate(async (cmd) => {
        const harness = window.testHarness;
        if (!harness) {
            throw new Error('Test harness not initialized');
        }
        return await harness.shellEval(cmd);
    }, command);

    return result as { output: string; success: boolean; error?: string };
}

// Helper to write a file via the sandbox MCP tool (async)
async function writeFile(page: Page, path: string, content: string): Promise<void> {
    await page.evaluate(async ({ path, content }) => {
        const harness = window.testHarness;
        if (!harness) {
            throw new Error('Test harness not initialized');
        }
        await harness.writeFile(path, content);
    }, { path, content });
}

// Helper to read a file via the sandbox MCP tool (async)
async function readFile(page: Page, path: string): Promise<string> {
    const result = await page.evaluate(async (path) => {
        const harness = window.testHarness;
        if (!harness) {
            throw new Error('Test harness not initialized');
        }
        return await harness.readFile(path);
    }, path);

    return result as string;
}

test.describe('WASM Core Functionality', () => {
    test.beforeEach(async ({ page }) => {
        await page.goto('/wasm-test.html');
        await page.waitForFunction(() => {
            return window.testHarness?.ready === true;
        }, { timeout: 30000 });
    });

    test('tsx can execute console.log', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "console.log(\'Hello WASM\')"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('Hello WASM');
    });

    test('tsx supports TypeScript syntax', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "const add = (a: number, b: number): number => a + b; console.log(add(2, 3))"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('5');
    });

    test('tsx supports top-level await', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "const x = await Promise.resolve(42); console.log(x)"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('42');
    });
});

test.describe('WASM Path Module', () => {
    test.beforeEach(async ({ page }) => {
        await page.goto('/wasm-test.html');
        await page.waitForFunction(() => {
            return window.testHarness?.ready === true;
        }, { timeout: 30000 });
    });

    test('path.join works correctly', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "console.log(path.join(\'/a\', \'b\', \'c\'))"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('/a/b/c');
    });

    test('path.dirname extracts directory', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "console.log(path.dirname(\'/a/b/file.txt\'))"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('/a/b');
    });

    test('path.basename extracts filename', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "console.log(path.basename(\'/a/b/file.txt\'))"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('file.txt');
    });

    test('path.extname extracts extension', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "console.log(path.extname(\'/a/b/file.txt\'))"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('.txt');
    });

    test('path.normalize handles ../', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "console.log(path.normalize(\'/a/b/../c\'))"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('/a/c');
    });
});

test.describe('WASM Buffer Module', () => {
    test.beforeEach(async ({ page }) => {
        await page.goto('/wasm-test.html');
        await page.waitForFunction(() => {
            return window.testHarness?.ready === true;
        }, { timeout: 30000 });
    });

    test('Buffer.from string works', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "console.log(Buffer.from(\'hello\').toString())"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('hello');
    });

    test('Buffer.from hex encoding works', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "console.log(Buffer.from(\'68656c6c6f\', \'hex\').toString())"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('hello');
    });

    test('Buffer.toString base64 works', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "console.log(Buffer.from(\'hello\').toString(\'base64\'))"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('aGVsbG8=');
    });

    test('Buffer.isBuffer works', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "console.log(Buffer.isBuffer(Buffer.from(\'a\')))"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('true');
    });
});

test.describe('WASM URL Module', () => {
    test.beforeEach(async ({ page }) => {
        await page.goto('/wasm-test.html');
        await page.waitForFunction(() => {
            return window.testHarness?.ready === true;
        }, { timeout: 30000 });
    });

    test('URL parsing works', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "console.log(new URL(\'https://example.com/path\').hostname)"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('example.com');
    });

    test('URLSearchParams works', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "console.log(new URL(\'https://example.com?a=1\').searchParams.get(\'a\'))"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('1');
    });

    test('URL origin works', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "console.log(new URL(\'https://example.com:8080/path\').origin)"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('https://example.com:8080');
    });
});

test.describe('WASM Encoding Module', () => {
    test.beforeEach(async ({ page }) => {
        await page.goto('/wasm-test.html');
        await page.waitForFunction(() => {
            return window.testHarness?.ready === true;
        }, { timeout: 30000 });
    });

    test('TextEncoder works', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "console.log(new TextEncoder().encode(\'hello\').length)"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('5');
    });

    test('TextDecoder works', async ({ page }) => {
        const result = await shellEval(page, `tsx -e "
            const enc = new TextEncoder();
            const dec = new TextDecoder();
            console.log(dec.decode(enc.encode('hello')));
        "`);
        expect(result.success).toBe(true);
        expect(result.output).toContain('hello');
    });

    test('btoa works', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "console.log(btoa(\'hello\'))"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('aGVsbG8=');
    });

    test('atob works', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "console.log(atob(\'aGVsbG8=\'))"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('hello');
    });
});

test.describe('WASM Async FS (fs.promises)', () => {
    test.beforeEach(async ({ page }) => {
        await page.goto('/wasm-test.html');
        await page.waitForFunction(() => {
            return window.testHarness?.ready === true;
        }, { timeout: 30000 });
    });

    test('fs.writeFileSync and readFileSync work', async ({ page }) => {
        // Write file using sync API
        const writeResult = await shellEval(page, 'tsx -e "fs.writeFileSync(\\"/sync-test.txt\\", \\"sync content\\"); console.log(\\"write done\\");"');
        expect(writeResult.success).toBe(true);
        expect(writeResult.output).toContain('write done');

        // Read file back using sync API (separate command to avoid buffering issues)
        const readResult = await shellEval(page, 'tsx -e "console.log(fs.readFileSync(\\"/sync-test.txt\\"));"');
        expect(readResult.success).toBe(true);
        expect(readResult.output).toContain('sync content');

        // Cleanup
        await shellEval(page, 'rm /sync-test.txt');
    });

    test('fs.promises.writeFile and readFile work', async ({ page }) => {
        const result = await shellEval(page, 'tsx -e "await fs.promises.writeFile(\\"/tmp/async-test.txt\\", \\"async content\\"); console.log(await fs.promises.readFile(\\"/tmp/async-test.txt\\"));"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('async content');
    });




    test('fs.promises.readdir works', async ({ page }) => {
        const result = await shellEval(page, `tsx -e "
            const entries = await fs.promises.readdir('/');
            console.log('isArray:', Array.isArray(entries));
        "`);
        expect(result.success).toBe(true);
        expect(result.output).toContain('isArray: true');
    });

    test('fs.promises.mkdir and rmdir work', async ({ page }) => {
        const result = await shellEval(page, `tsx -e "
            await fs.promises.mkdir('/test-async-dir');
            const stat = await fs.promises.stat('/test-async-dir');
            console.log('created:', stat.isDirectory());
            await fs.promises.rmdir('/test-async-dir');
        "`);
        expect(result.success).toBe(true);
        expect(result.output).toContain('created: true');
    });
});


test.describe('MCP Tools', () => {
    test.beforeEach(async ({ page }) => {
        await page.goto('/wasm-test.html');
        await page.waitForFunction(() => {
            return window.testHarness?.ready === true;
        }, { timeout: 30000 });
    });

    test('write_file and read_file tools work', async ({ page }) => {
        await writeFile(page, '/mcp-test.txt', 'hello mcp');
        const content = await readFile(page, '/mcp-test.txt');
        expect(content).toBe('hello mcp');
    });

    test('shell_eval can run echo', async ({ page }) => {
        const result = await shellEval(page, 'echo "Hello from shell"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('Hello from shell');
    });
});

test.describe('Shell Glob Expansion', () => {
    test.beforeEach(async ({ page }) => {
        await page.goto('/wasm-test.html');
        await page.waitForFunction(() => {
            return window.testHarness?.ready === true;
        }, { timeout: 30000 });
    });

    test('glob * expands to matching files', async ({ page }) => {
        // Create test files
        await writeFile(page, '/globtest/file1.txt', 'content1');
        await writeFile(page, '/globtest/file2.txt', 'content2');
        await writeFile(page, '/globtest/other.rs', 'rust');

        // Test glob expansion with *.txt
        const result = await shellEval(page, 'echo /globtest/*.txt');
        expect(result.success).toBe(true);
        expect(result.output).toContain('file1.txt');
        expect(result.output).toContain('file2.txt');
        expect(result.output).not.toContain('other.rs');
    });

    test('glob ? expands to single character match', async ({ page }) => {
        // Create test files
        await writeFile(page, '/globtest2/a1.txt', '');
        await writeFile(page, '/globtest2/a2.txt', '');
        await writeFile(page, '/globtest2/b1.txt', '');

        // Test ? pattern
        const result = await shellEval(page, 'echo /globtest2/a?.txt');
        expect(result.success).toBe(true);
        expect(result.output).toContain('a1.txt');
        expect(result.output).toContain('a2.txt');
        expect(result.output).not.toContain('b1.txt');
    });

    test('rm with glob deletes matching files', async ({ page }) => {
        // Create test files
        await writeFile(page, '/rmtest/del1.txt', 'delete me');
        await writeFile(page, '/rmtest/del2.txt', 'delete me too');
        await writeFile(page, '/rmtest/keep.rs', 'keep this');

        // Delete only .txt files using glob
        const rmResult = await shellEval(page, 'rm /rmtest/*.txt');
        expect(rmResult.success).toBe(true);

        // Verify .txt files are gone
        const lsResult = await shellEval(page, 'ls /rmtest');
        expect(lsResult.success).toBe(true);
        expect(lsResult.output).not.toContain('del1.txt');
        expect(lsResult.output).not.toContain('del2.txt');
        expect(lsResult.output).toContain('keep.rs');
    });
});

test.describe('Archive Commands', () => {
    test.beforeEach(async ({ page }) => {
        await page.goto('/wasm-test.html');
        await page.waitForFunction(() => {
            return window.testHarness?.ready === true;
        }, { timeout: 30000 });
    });

    test('gzip compresses and gunzip decompresses', async ({ page }) => {
        // Create a test file
        await writeFile(page, '/gztest/file.txt', 'hello gzip world');

        // Compress with gzip  
        const gzipResult = await shellEval(page, 'gzip -k /gztest/file.txt');
        expect(gzipResult.success).toBe(true);

        // Verify .gz file exists
        const lsResult = await shellEval(page, 'ls /gztest');
        expect(lsResult.success).toBe(true);
        expect(lsResult.output).toContain('file.txt.gz');

        // Decompress with zcat to stdout
        const zcatResult = await shellEval(page, 'zcat /gztest/file.txt.gz');
        expect(zcatResult.success).toBe(true);
        expect(zcatResult.output).toContain('hello gzip world');
    });

    test('tar creates and extracts archives', async ({ page }) => {
        // Create test files
        await writeFile(page, '/tartest/src/a.txt', 'file a');
        await writeFile(page, '/tartest/src/b.txt', 'file b');

        // Create tar archive
        const createResult = await shellEval(page, 'cd /tartest/src && tar -cvf /tartest/archive.tar a.txt b.txt');
        expect(createResult.success).toBe(true);

        // List archive contents
        const listResult = await shellEval(page, 'tar -tvf /tartest/archive.tar');
        expect(listResult.success).toBe(true);
        expect(listResult.output).toContain('a.txt');
        expect(listResult.output).toContain('b.txt');
    });

    test('zip creates and unzip extracts', async ({ page }) => {
        // Create test file
        await writeFile(page, '/ziptest/file.txt', 'zip content here');

        // Create zip archive
        const zipResult = await shellEval(page, 'cd /ziptest && zip archive.zip file.txt');
        expect(zipResult.success).toBe(true);

        // List zip contents
        const listResult = await shellEval(page, 'unzip -l /ziptest/archive.zip');
        expect(listResult.success).toBe(true);
        expect(listResult.output).toContain('file.txt');
    });
});

test.describe('Git Commands', () => {
    test.beforeEach(async ({ page }) => {
        await page.goto('/wasm-test.html');
        await page.waitForFunction(() => {
            return window.testHarness?.ready === true;
        }, { timeout: 30000 });
    });

    test('git init creates a repository', async ({ page }) => {
        // First git operation triggers lazy loading of the Go WASM binary (~17MB)
        test.slow();

        // Create directory and init
        const mkdirResult = await shellEval(page, 'mkdir -p /gitrepo');
        expect(mkdirResult.success).toBe(true);

        const initResult = await shellEval(page, 'cd /gitrepo && git init');
        expect(initResult.success).toBe(true);
        expect(initResult.output).toContain('Initialized');

        // Verify .git directory exists
        const lsResult = await shellEval(page, 'ls -a /gitrepo');
        expect(lsResult.success).toBe(true);
        expect(lsResult.output).toContain('.git');
    });

    test('git status shows repository state', async ({ page }) => {
        test.slow();

        // Create and init repo
        await shellEval(page, 'mkdir -p /gitrepo2');
        await shellEval(page, 'cd /gitrepo2 && git init');

        // Check status
        const statusResult = await shellEval(page, 'cd /gitrepo2 && git status');
        expect(statusResult.success).toBe(true);
        expect(statusResult.output).toContain('On branch');
    });

    test('git help shows available commands', async ({ page }) => {
        const helpResult = await shellEval(page, 'git --help');
        expect(helpResult.success).toBe(true);
        expect(helpResult.output).toContain('init');
        expect(helpResult.output).toContain('status');
        expect(helpResult.output).toContain('commit');
    });
});

test.describe('OPFS SyncAccessHandle Primitives', () => {
    // These tests exercise OPFS directly in a dedicated Worker,
    // completely bypassing the WASM/WASI/shim stack.
    // This validates our assumptions about browser OPFS behavior.

    /**
     * Run an OPFS test inside a dedicated Worker via page.evaluate().
     * The function string is executed in a Worker context with access to
     * navigator.storage.getDirectory() and SyncAccessHandle APIs.
     */
    async function opfsWorkerTest(page: Page, testFn: string): Promise<{ ok: boolean; result: string; error?: string }> {
        return await page.evaluate(async (fnBody) => {
            // Create a blob URL for a Worker that runs the test
            const workerCode = `
                self.onmessage = async function() {
                    try {
                        const root = await navigator.storage.getDirectory();
                        // Helper to get/create a test file
                        async function getTestFile(name, create = true) {
                            if (create) {
                                return await root.getFileHandle(name, { create: true });
                            }
                            return await root.getFileHandle(name);
                        }
                        // Helper to remove a test file
                        async function removeTestFile(name) {
                            try { await root.removeEntry(name); } catch(e) {}
                        }
                        ${fnBody}
                    } catch(e) {
                        self.postMessage({ ok: false, result: '', error: e.message || String(e) });
                    }
                };
            `;
            const blob = new Blob([workerCode], { type: 'application/javascript' });
            const url = URL.createObjectURL(blob);
            const worker = new Worker(url);

            return new Promise<{ ok: boolean; result: string; error?: string }>((resolve) => {
                const timeout = setTimeout(() => {
                    worker.terminate();
                    resolve({ ok: false, result: '', error: 'Worker timeout (10s)' });
                }, 10000);

                worker.onmessage = (e) => {
                    clearTimeout(timeout);
                    worker.terminate();
                    URL.revokeObjectURL(url);
                    resolve(e.data);
                };

                worker.onerror = (e) => {
                    clearTimeout(timeout);
                    worker.terminate();
                    URL.revokeObjectURL(url);
                    resolve({ ok: false, result: '', error: e.message || 'Worker error' });
                };

                worker.postMessage('run');
            });
        }, testFn);
    }

    test('SyncAccessHandle write and read at offset 0', async ({ page }) => {
        await page.goto('/wasm-test.html');
        const r = await opfsWorkerTest(page, `
            await removeTestFile('test-basic.dat');
            const fh = await getTestFile('test-basic.dat');
            const handle = await fh.createSyncAccessHandle();
            const data = new TextEncoder().encode('HELLO');
            handle.write(data, { at: 0 });
            handle.flush();
            const readBuf = new Uint8Array(5);
            handle.read(readBuf, { at: 0 });
            handle.close();
            await removeTestFile('test-basic.dat');
            const got = new TextDecoder().decode(readBuf);
            self.postMessage({ ok: got === 'HELLO', result: got });
        `);
        expect(r.error).toBeUndefined();
        expect(r.ok).toBe(true);
        expect(r.result).toBe('HELLO');
    });

    test('SyncAccessHandle write at two offsets preserves both', async ({ page }) => {
        await page.goto('/wasm-test.html');
        const r = await opfsWorkerTest(page, `
            await removeTestFile('test-offsets.dat');
            const fh = await getTestFile('test-offsets.dat');
            const handle = await fh.createSyncAccessHandle();
            // Write "AAAA" at offset 0
            handle.write(new TextEncoder().encode('AAAA'), { at: 0 });
            // Write "BBBB" at offset 4096
            handle.write(new TextEncoder().encode('BBBB'), { at: 4096 });
            handle.flush();
            // Read back both
            const buf1 = new Uint8Array(4);
            handle.read(buf1, { at: 0 });
            const buf2 = new Uint8Array(4);
            handle.read(buf2, { at: 4096 });
            handle.close();
            await removeTestFile('test-offsets.dat');
            const got1 = new TextDecoder().decode(buf1);
            const got2 = new TextDecoder().decode(buf2);
            self.postMessage({ ok: got1 === 'AAAA' && got2 === 'BBBB', result: got1 + '|' + got2 });
        `);
        expect(r.error).toBeUndefined();
        expect(r.ok).toBe(true);
        expect(r.result).toBe('AAAA|BBBB');
    });

    test('SyncAccessHandle getSize reflects writes', async ({ page }) => {
        await page.goto('/wasm-test.html');
        const r = await opfsWorkerTest(page, `
            await removeTestFile('test-size.dat');
            const fh = await getTestFile('test-size.dat');
            const handle = await fh.createSyncAccessHandle();
            // Write 8192 bytes
            handle.write(new Uint8Array(8192), { at: 0 });
            handle.flush();
            const size = handle.getSize();
            handle.close();
            await removeTestFile('test-size.dat');
            self.postMessage({ ok: size === 8192, result: 'size=' + size });
        `);
        expect(r.error).toBeUndefined();
        expect(r.ok).toBe(true);
        expect(r.result).toBe('size=8192');
    });

    test('SyncAccessHandle close and reopen preserves data', async ({ page }) => {
        await page.goto('/wasm-test.html');
        const r = await opfsWorkerTest(page, `
            await removeTestFile('test-reopen.dat');
            const fh1 = await getTestFile('test-reopen.dat');
            const h1 = await fh1.createSyncAccessHandle();
            // Write two pages
            const page1 = new Uint8Array(4096);
            page1.set(new TextEncoder().encode('PAGE1'));
            h1.write(page1, { at: 0 });
            const page2 = new Uint8Array(4096);
            page2.set(new TextEncoder().encode('PAGE2'));
            h1.write(page2, { at: 4096 });
            h1.flush();
            h1.close();

            // Reopen and read
            const fh2 = await getTestFile('test-reopen.dat', false);
            const h2 = await fh2.createSyncAccessHandle();
            const size = h2.getSize();
            const r1 = new Uint8Array(5);
            h2.read(r1, { at: 0 });
            const r2 = new Uint8Array(5);
            h2.read(r2, { at: 4096 });
            h2.close();
            await removeTestFile('test-reopen.dat');
            const s1 = new TextDecoder().decode(r1);
            const s2 = new TextDecoder().decode(r2);
            self.postMessage({ ok: s1 === 'PAGE1' && s2 === 'PAGE2' && size === 8192, result: s1 + '|' + s2 + '|size=' + size });
        `);
        expect(r.error).toBeUndefined();
        expect(r.ok).toBe(true);
        expect(r.result).toBe('PAGE1|PAGE2|size=8192');
    });

    test('SyncAccessHandle overwrite at offset preserves other data', async ({ page }) => {
        await page.goto('/wasm-test.html');
        const r = await opfsWorkerTest(page, `
            await removeTestFile('test-overwrite.dat');
            const fh = await getTestFile('test-overwrite.dat');
            const handle = await fh.createSyncAccessHandle();
            // Write 8192 zeros
            handle.write(new Uint8Array(8192), { at: 0 });
            // Write "PAGE1" at 0 and "PAGE2" at 4096
            handle.write(new TextEncoder().encode('PAGE1'), { at: 0 });
            handle.write(new TextEncoder().encode('PAGE2'), { at: 4096 });
            handle.flush();
            // Overwrite only offset 4096
            handle.write(new TextEncoder().encode('XXXXX'), { at: 4096 });
            handle.flush();
            // Read both back
            const b1 = new Uint8Array(5);
            handle.read(b1, { at: 0 });
            const b2 = new Uint8Array(5);
            handle.read(b2, { at: 4096 });
            handle.close();
            await removeTestFile('test-overwrite.dat');
            const s1 = new TextDecoder().decode(b1);
            const s2 = new TextDecoder().decode(b2);
            self.postMessage({ ok: s1 === 'PAGE1' && s2 === 'XXXXX', result: s1 + '|' + s2 });
        `);
        expect(r.error).toBeUndefined();
        expect(r.ok).toBe(true);
        expect(r.result).toBe('PAGE1|XXXXX');
    });

    test('SyncAccessHandle simulates SQLite write pattern', async ({ page }) => {
        // Mimics what SQLite does: write header page, write data page,
        // close, reopen, read back
        await page.goto('/wasm-test.html');
        const r = await opfsWorkerTest(page, `
            await removeTestFile('test-sqlite-pattern.db');
            const fh1 = await getTestFile('test-sqlite-pattern.db');
            const h1 = await fh1.createSyncAccessHandle();

            // Write SQLite-like header (page 1, 4096 bytes)
            const headerPage = new Uint8Array(4096);
            const magic = new TextEncoder().encode('SQLite format 3');
            headerPage.set(magic, 0);
            headerPage[0x10] = 0x10; // page_size = 4096 (big-endian)
            headerPage[0x11] = 0x00;
            headerPage[0x1C] = 0x00; // page_count = 2 (big-endian)
            headerPage[0x1D] = 0x00;
            headerPage[0x1E] = 0x00;
            headerPage[0x1F] = 0x02;
            h1.write(headerPage, { at: 0 });

            // Write data page (page 2, 4096 bytes)
            const dataPage = new Uint8Array(4096);
            dataPage.set(new TextEncoder().encode('TABLE_DATA_HERE'), 0);
            h1.write(dataPage, { at: 4096 });
            h1.flush();
            h1.close();

            // Reopen and verify
            const fh2 = await getTestFile('test-sqlite-pattern.db', false);
            const h2 = await fh2.createSyncAccessHandle();
            const size = h2.getSize();

            // Read header
            const hdr = new Uint8Array(32);
            h2.read(hdr, { at: 0 });
            const magicRead = new TextDecoder().decode(hdr.slice(0, 15));
            const pageCount = (hdr[0x1C] << 24) | (hdr[0x1D] << 16) | (hdr[0x1E] << 8) | hdr[0x1F];

            // Read data page
            const dp = new Uint8Array(15);
            h2.read(dp, { at: 4096 });
            const dataRead = new TextDecoder().decode(dp);
            h2.close();
            await removeTestFile('test-sqlite-pattern.db');

            const allOk = size === 8192 && magicRead === 'SQLite format 3' && pageCount === 2 && dataRead === 'TABLE_DATA_HERE';
            self.postMessage({ ok: allOk, result: 'size=' + size + ' magic=' + magicRead + ' pages=' + pageCount + ' data=' + dataRead });
        `);
        expect(r.error).toBeUndefined();
        expect(r.ok).toBe(true);
        expect(r.result).toContain('size=8192');
        expect(r.result).toContain('magic=SQLite format 3');
        expect(r.result).toContain('pages=2');
        expect(r.result).toContain('data=TABLE_DATA_HERE');
    });
});

test.describe('OPFS via WASI Shim (through Rust)', () => {
    // These tests exercise the OPFS shim through the WASM/WASI layer
    // using shell commands that work (echo, cat, wc, xxd, writeFile/readFile).
    // This validates the shim's stat, write, read, and cross-invocation persistence.

    test.beforeEach(async ({ page }) => {
        await page.goto('/wasm-test.html');
        await page.waitForFunction(() => {
            return window.testHarness?.ready === true;
        }, { timeout: 30000 });
    });

    test('text file persists across shell invocations', async ({ page }) => {
        const write = await shellEval(page, `echo "hello world" > /tmp/opfs-text.txt`);
        expect(write.success).toBe(true);
        const read = await shellEval(page, `cat /tmp/opfs-text.txt`);
        expect(read.success).toBe(true);
        expect(read.output.trim()).toBe('hello world');
    });

    test('writeFile/readFile MCP tools persist content', async ({ page }) => {
        await writeFile(page, '/tmp/opfs-mcp.txt', 'mcp content');
        const content = await readFile(page, '/tmp/opfs-mcp.txt');
        expect(content).toBe('mcp content');
    });

    test('wc -c reports correct file size', async ({ page }) => {
        // Write a known-size file via echo (no trailing newline surprise)
        const write = await shellEval(page, `echo -n "${'x'.repeat(100)}" > /tmp/opfs-wc.txt`);
        expect(write.success).toBe(true);
        const wc = await shellEval(page, `wc -c < /tmp/opfs-wc.txt`);
        expect(wc.success).toBe(true);
        expect(parseInt(wc.output.trim())).toBe(100);
    });

    test('file created by sqlite3 has correct header via xxd', async ({ page }) => {
        // Create a database, then verify the raw bytes on disk
        const create = await shellEval(page, `sqlite3 /tmp/opfs-header.db "CREATE TABLE t(x); INSERT INTO t VALUES(1)"`);
        expect(create.success).toBe(true);

        // Read the first 16 bytes — should be SQLite magic
        const xxd = await shellEval(page, `xxd -l 16 /tmp/opfs-header.db`);
        expect(xxd.success).toBe(true);
        expect(xxd.output).toContain('5351 4c69 7465 2066 6f72 6d61 7420 33');
    });

    test('large text file survives close and reopen', async ({ page }) => {
        // Write a file larger than a single OPFS page
        const bigContent = 'ABCDEFGH'.repeat(1024); // 8KB
        await writeFile(page, '/tmp/opfs-large.txt', bigContent);
        const read = await readFile(page, '/tmp/opfs-large.txt');
        expect(read.length).toBe(bigContent.length);
        expect(read).toBe(bigContent);
    });

    test('overwrite replaces file content', async ({ page }) => {
        await writeFile(page, '/tmp/opfs-overwrite.txt', 'first');
        await writeFile(page, '/tmp/opfs-overwrite.txt', 'second');
        const content = await readFile(page, '/tmp/opfs-overwrite.txt');
        expect(content).toBe('second');
    });

    test('sqlite3 file size grows with data', async ({ page }) => {
        // Create a database and check size
        await shellEval(page, `sqlite3 /tmp/opfs-grow.db "CREATE TABLE t(x TEXT)"`);
        const size1 = await shellEval(page, `wc -c < /tmp/opfs-grow.db`);
        expect(size1.success).toBe(true);
        const s1 = parseInt(size1.output.trim());

        // Insert enough data to force page allocation (>4088 usable bytes per page)
        await shellEval(page, `sqlite3 /tmp/opfs-grow.db "INSERT INTO t VALUES('${'x'.repeat(8000)}')"`);
        const size2 = await shellEval(page, `wc -c < /tmp/opfs-grow.db`);
        expect(size2.success).toBe(true);
        const s2 = parseInt(size2.output.trim());
        expect(s2).toBeGreaterThan(s1);
    });
});

test.describe('SQLite3 (rusqlite WASM)', () => {
    test.beforeEach(async ({ page }) => {
        await page.goto('/wasm-test.html');
        await page.waitForFunction(() => {
            return window.testHarness?.ready === true;
        }, { timeout: 30000 });
    });

    test('sqlite3 executes inline SQL on :memory:', async ({ page }) => {
        const result = await shellEval(page, 'sqlite3 "SELECT 1 + 2"');
        expect(result.success).toBe(true);
        expect(result.output.trim()).toBe('3');
    });

    test('sqlite3 creates table and inserts rows', async ({ page }) => {
        const result = await shellEval(page, `sqlite3 "CREATE TABLE t(id INTEGER, name TEXT); INSERT INTO t VALUES(1,'alice'),(2,'bob'); SELECT * FROM t"`);
        expect(result.success).toBe(true);
        expect(result.output).toContain('1|alice');
        expect(result.output).toContain('2|bob');
    });

    test('sqlite3 supports multiple statements', async ({ page }) => {
        const result = await shellEval(page, `sqlite3 "CREATE TABLE nums(n); INSERT INTO nums VALUES(10),(20),(30); SELECT SUM(n) FROM nums"`);
        expect(result.success).toBe(true);
        expect(result.output.trim()).toBe('60');
    });

    test('sqlite3 handles NULL values', async ({ page }) => {
        const result = await shellEval(page, `sqlite3 "SELECT NULL, 42, 'text'"`);
        expect(result.success).toBe(true);
        expect(result.output.trim()).toBe('|42|text');
    });

    test('sqlite3 supports aggregate functions', async ({ page }) => {
        const result = await shellEval(page, `sqlite3 "CREATE TABLE scores(v REAL); INSERT INTO scores VALUES(1.5),(2.5),(3.0); SELECT COUNT(*), AVG(v), MIN(v), MAX(v) FROM scores"`);
        expect(result.success).toBe(true);
        const cols = result.output.trim().split('|');
        expect(cols[0]).toBe('3');
        expect(parseFloat(cols[1])).toBeCloseTo(2.333, 2);
        expect(parseFloat(cols[2])).toBe(1.5);
        expect(parseFloat(cols[3])).toBe(3.0);
    });

    test('sqlite3 reads SQL from stdin via pipe', async ({ page }) => {
        const result = await shellEval(page, `echo "SELECT 'piped'" | sqlite3`);
        expect(result.success).toBe(true);
        expect(result.output.trim()).toBe('piped');
    });

    test('sqlite3 CREATE + INSERT on file-backed db', async ({ page }) => {
        const r = await shellEval(page, `sqlite3 /tmp/test-ci.db "CREATE TABLE t(x); INSERT INTO t VALUES(1)"`);
        expect(r.success).toBe(true);
    });

    test('sqlite3 file size after CREATE TABLE', async ({ page }) => {
        // Create a database with just a table
        const create = await shellEval(page, `sqlite3 /tmp/test-size.db "CREATE TABLE t(x)"`);
        expect(create.success).toBe(true);

        // Check the file size via wc
        const wc = await shellEval(page, `wc -c < /tmp/test-size.db`);
        console.log('wc -c:', JSON.stringify(wc));

        // Check via xxd (reads raw binary data)
        const xxd = await shellEval(page, `xxd -l 32 /tmp/test-size.db`);
        console.log('xxd:', xxd.output);

        // Check via ls -l
        const ls = await shellEval(page, `ls -l /tmp/test-size.db`);
        console.log('ls -l:', ls.output);

        // The file should have data — xxd should show SQLite magic
        expect(xxd.success).toBe(true);
        expect(xxd.output).toContain('5351 4c69'); // "SQLi" in hex
    });

    test('sqlite3 persists data to a file-backed database', async ({ page }) => {
        // Write data in first invocation
        const write = await shellEval(page, `sqlite3 /tmp/test.db "CREATE TABLE IF NOT EXISTS kv(k TEXT, v TEXT); INSERT INTO kv VALUES('hello','world'); SELECT v FROM kv WHERE k='hello'"`);
        expect(write.success).toBe(true);
        expect(write.output.trim()).toBe('world');

        // Read back in a separate invocation
        const read = await shellEval(page, `sqlite3 /tmp/test.db "SELECT v FROM kv WHERE k='hello'"`);
        expect(read.success).toBe(true);
        expect(read.output.trim()).toBe('world');
    });

    test('sqlite3 reports error on invalid SQL', async ({ page }) => {
        const result = await shellEval(page, 'sqlite3 "NOT VALID SQL"');
        expect(result.success).toBe(false);
    });

    test('sqlite3 supports FTS5 full-text search', async ({ page }) => {
        const result = await shellEval(page, `sqlite3 "CREATE VIRTUAL TABLE docs USING fts5(content); INSERT INTO docs VALUES('the quick brown fox'),('lazy dog jumps'); SELECT content FROM docs WHERE docs MATCH 'quick'"`);
        expect(result.success).toBe(true);
        expect(result.output.trim()).toBe('the quick brown fox');
    });

    test('sqlite3 supports JSON functions', async ({ page }) => {
        const result = await shellEval(page, `sqlite3 "SELECT json_extract('{\"a\":1,\"b\":2}', '$.b')"`);
        expect(result.success).toBe(true);
        expect(result.output.trim()).toBe('2');
    });
});

test.describe('WASM Stripe CLI (Go Component)', () => {
    test.beforeEach(async ({ page }) => {
        await page.goto('/wasm-test.html');
        await page.waitForFunction(() => {
            return window.testHarness?.ready === true;
        }, { timeout: 30000 });
    });

    test('stripe help dispatches to Go CLI', async ({ page }) => {
        // Longer timeout for first load + compilation of 37MB Go WASM binary
        test.setTimeout(120000);
        const result = await shellEval(page, 'stripe help');
        console.log('stripe help result:', JSON.stringify(result));
        // Skip if stripe module wasn't built (local dev without Go WASM)
        const combined = (result.output || '') + (result.error || '');
        if (combined.includes('command not found') || combined.includes('not loaded')) {
            test.skip(true, 'Stripe Go WASM module not available');
        }
        // The Go CLI should exit successfully and produce real help output
        expect(result.success).toBe(true);
        expect(result.output).toContain('Usage:');
        expect(result.output).toContain('stripe [command]');
        expect(result.output).toContain('login');
    });
});
