/**
 * Python (Pyodide) E2E Tests
 *
 * Tests the Pyodide-based Python runtime running in a real browser environment.
 * Pyodide is lazy-loaded on first use (~5-10s), so the first test in each
 * describe block uses test.slow() to extend the timeout.
 *
 * OPFS is mounted into Pyodide's Emscripten FS at /home/user, so files
 * created by the shell are accessible to Python and vice versa.
 */

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

// Helper to write a file via the sandbox MCP tool
async function writeFile(page: Page, path: string, content: string): Promise<void> {
    await page.evaluate(async ({ path, content }) => {
        const harness = window.testHarness;
        if (!harness) {
            throw new Error('Test harness not initialized');
        }
        await harness.writeFile(path, content);
    }, { path, content });
}

test.describe('Python (Pyodide)', () => {
    test.beforeEach(async ({ page }) => {
        await page.goto('/wasm-test.html');
        await page.waitForFunction(() => {
            return window.testHarness?.ready === true;
        }, { timeout: 30000 });
    });

    test('python3 --version reports CPython version', async ({ page }) => {
        // First invocation lazy-loads Pyodide (~9MB WASM + stdlib)
        test.slow();

        const result = await shellEval(page, 'python3 --version');
        expect(result.success).toBe(true);
        expect(result.output).toMatch(/Python 3\.\d+/);
    });

    test('python3 -c executes inline code', async ({ page }) => {
        test.slow();

        const result = await shellEval(page, 'python3 -c "print(\'Hello from Python\')"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('Hello from Python');
    });

    test('python3 -c supports arithmetic', async ({ page }) => {
        test.slow();

        const result = await shellEval(page, 'python3 -c "print(2 + 3)"');
        expect(result.success).toBe(true);
        expect(result.output.trim()).toContain('5');
    });

    test('python3 -c can import stdlib modules', async ({ page }) => {
        test.slow();

        const result = await shellEval(page, 'python3 -c "import json; print(json.dumps({\'a\': 1, \'b\': 2}))"');
        expect(result.success).toBe(true);
        expect(result.output).toContain('"a"');
        expect(result.output).toContain('"b"');
    });

    test('python3 -c handles errors gracefully', async ({ page }) => {
        test.slow();

        const result = await shellEval(page, 'python3 -c "1/0"');
        expect(result.success).toBe(false);
        expect(result.output + (result.error || '')).toMatch(/ZeroDivisionError/);
    });

    test('python3 runs a script file', async ({ page }) => {
        test.slow();

        await writeFile(page, '/test_script.py', 'import sys\nprint(f"args: {sys.argv}")\nprint("script works")');

        const result = await shellEval(page, 'python3 /test_script.py arg1 arg2');
        expect(result.success).toBe(true);
        expect(result.output).toContain('script works');
    });

    test('python3 reports missing script file', async ({ page }) => {
        test.slow();

        const result = await shellEval(page, 'python3 /nonexistent.py');
        expect(result.success).toBe(false);
        expect(result.output + (result.error || '')).toContain('No such file');
    });

    test('python alias works same as python3', async ({ page }) => {
        test.slow();

        const result = await shellEval(page, 'python -c "print(42)"');
        expect(result.success).toBe(true);
        expect(result.output.trim()).toContain('42');
    });

    test('python3 -m runs a module', async ({ page }) => {
        test.slow();

        const result = await shellEval(page, 'python3 -m json.tool --help');
        expect(result.success).toBe(true);
    });

    test('python3 preserves state across invocations via shared interpreter', async ({ page }) => {
        test.slow();

        // The Pyodide instance is cached, so globals persist
        await shellEval(page, 'python3 -c "import builtins; builtins._test_val = 123"');
        const result = await shellEval(page, 'python3 -c "import builtins; print(builtins._test_val)"');
        expect(result.success).toBe(true);
        expect(result.output.trim()).toContain('123');
    });
});

test.describe('pip (micropip)', () => {
    test.beforeEach(async ({ page }) => {
        await page.goto('/wasm-test.html');
        await page.waitForFunction(() => {
            return window.testHarness?.ready === true;
        }, { timeout: 30000 });
    });

    test('pip --help shows usage', async ({ page }) => {
        test.slow();

        const result = await shellEval(page, 'pip --help');
        expect(result.success).toBe(true);
        expect(result.output).toContain('install');
    });

    test('pip list shows installed packages', async ({ page }) => {
        test.slow();

        const result = await shellEval(page, 'pip list');
        expect(result.success).toBe(true);
        // micropip should always be available
        expect(result.output).toMatch(/micropip/i);
    });

    test('pip install installs a pure-python package', async ({ page }) => {
        test.slow();

        // Install a small pure-python package
        const installResult = await shellEval(page, 'pip install six');
        expect(installResult.success).toBe(true);
        expect(installResult.output).toContain('Successfully installed');

        // Verify it's importable
        const useResult = await shellEval(page, 'python3 -c "import six; print(six.__version__)"');
        expect(useResult.success).toBe(true);
    });
});
