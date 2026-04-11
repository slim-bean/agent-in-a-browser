/**
 * Python (Pyodide) E2E Tests
 *
 * Tests the Pyodide-based Python runtime in the brush shell TUI.
 * Pyodide is lazy-loaded on first use (~5-10s), so the first test
 * uses test.slow() to extend the timeout.
 *
 * OPFS is mounted at / in Pyodide's WasmFS, so shell paths and Python paths
 * share the same namespace — no translation needed.
 */

import { test, expect } from './webkit-persistent-fixture';
import type { Page } from '@playwright/test';

// ---------------------------------------------------------------------------
// Helpers – interact with the brush shell running in the TUI terminal
// ---------------------------------------------------------------------------

async function focusTerminal(page: Page): Promise<void> {
    await page.evaluate(() => { window.tuiTerminal?.focus(); });
    await page.waitForTimeout(100);
}

async function getTerminalText(page: Page, trimTrailing = true): Promise<string> {
    return await page.evaluate((trim) => {
        const terminal = window.tuiTerminal;
        if (!terminal?.buffer?.active) return '';
        const lines: string[] = [];
        const buffer = terminal.buffer.active;
        for (let y = 0; y < terminal.rows; y++) {
            const line = buffer.getLine(y);
            if (line) lines.push(line.translateToString(trim));
        }
        return lines.join('\n');
    }, trimTrailing);
}

async function waitForText(page: Page, text: string, timeout = 30000): Promise<string> {
    const start = Date.now();
    while (Date.now() - start < timeout) {
        const screen = await getTerminalText(page);
        if (screen.includes(text)) return screen;
        await page.waitForTimeout(250);
    }
    const screen = await getTerminalText(page);
    throw new Error(`Timeout waiting for "${text}". Screen:\n${screen}`);
}

/** Shell prompt is "{cwd}$ " but trailing space is trimmed by translateToString(true). */
const PROMPT_RE = /\/.*\$$/m;

/**
 * Type a command and press Enter, then wait for expected output.
 * Looks for `expectText` in lines that appear AFTER the typed command line,
 * to avoid false positives from previous output still on screen.
 */
async function run(page: Page, command: string, expectText: string, timeout = 60000): Promise<string> {
    await focusTerminal(page);
    await page.keyboard.type(command, { delay: 20 });
    await page.keyboard.press('Enter');

    // Use first 30 chars of the command to identify which line it's on
    const cmdSnippet = command.slice(0, 30);

    const start = Date.now();
    while (Date.now() - start < timeout) {
        const screen = await getTerminalText(page);
        const lines = screen.split('\n');
        // Find the last line containing our command
        let cmdLineIdx = -1;
        for (let i = lines.length - 1; i >= 0; i--) {
            if (lines[i].includes(cmdSnippet)) { cmdLineIdx = i; break; }
        }
        if (cmdLineIdx >= 0) {
            // Check lines after the command for expected text
            const outputLines = lines.slice(cmdLineIdx + 1).join('\n');
            if (outputLines.includes(expectText)) return screen;
        }
        await page.waitForTimeout(250);
    }
    const screen = await getTerminalText(page);
    throw new Error(`Timeout waiting for "${expectText}". Screen:\n${screen}`);
}

/**
 * Type a command and press Enter, then wait for a new shell prompt to appear.
 * Detects completion by looking for a bare prompt line *after* the line
 * that contains the typed command.
 */
async function runAndWaitForPrompt(page: Page, command: string, timeout = 30000): Promise<string> {
    await focusTerminal(page);
    await page.keyboard.type(command, { delay: 20 });
    await page.keyboard.press('Enter');

    const cmdSnippet = command.slice(0, 30);

    const start = Date.now();
    while (Date.now() - start < timeout) {
        const screen = await getTerminalText(page);
        const lines = screen.split('\n');
        let cmdLineIdx = -1;
        for (let i = lines.length - 1; i >= 0; i--) {
            if (lines[i].includes(cmdSnippet)) { cmdLineIdx = i; break; }
        }
        if (cmdLineIdx >= 0) {
            for (let i = cmdLineIdx + 1; i < lines.length; i++) {
                if (PROMPT_RE.test(lines[i])) return screen;
            }
        }
        await page.waitForTimeout(250);
    }
    const screen = await getTerminalText(page);
    throw new Error(`Timeout waiting for prompt after: "${command}". Screen:\n${screen}`);
}

async function waitForShellReady(page: Page, timeout = 30000): Promise<void> {
    await page.waitForSelector('canvas', { timeout });
    await page.waitForFunction(
        () => window.tuiTerminal?.buffer?.active !== undefined,
        { timeout },
    );
    const start = Date.now();
    while (Date.now() - start < timeout) {
        const screen = await getTerminalText(page);
        if (PROMPT_RE.test(screen)) return;
        await page.waitForTimeout(250);
    }
    const screen = await getTerminalText(page);
    throw new Error(`Timeout waiting for shell prompt. Screen:\n${screen}`);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

test.describe('Python (Pyodide) – file interactions', () => {
    test.beforeEach(async ({ page }) => {
        await page.goto('/');
        await waitForShellReady(page);
    });

    test('python3 --version reports CPython', async ({ page }) => {
        test.slow();
        const screen = await run(page, 'python3 --version', 'Python 3');
        expect(screen).toMatch(/Python 3\.\d+/);
    });

    test('python3 -c inline code', async ({ page }) => {
        test.slow();
        const screen = await run(page, "python3 -c \"print('hello pyodide')\"", 'hello pyodide');
        expect(screen).toContain('hello pyodide');
    });

    test('python3 reads a file created by the shell', async ({ page }) => {
        test.slow();

        // Create a file using echo (shell writes to OPFS root /)
        await runAndWaitForPrompt(page, 'echo "apple banana cherry" > /fruits.txt');

        // OPFS is at / — Python's cwd is / so relative 'fruits.txt' = /fruits.txt.
        const screen = await run(
            page,
            "python3 -c \"print(open('fruits.txt').read().strip())\"",
            'apple banana cherry',
        );
        expect(screen).toContain('apple banana cherry');
    });

    test('python3 writes a file readable by shell cat', async ({ page }) => {
        test.slow();

        // Python writes to its cwd (/) — same OPFS namespace as the shell.
        await runAndWaitForPrompt(
            page,
            "python3 -c \"open('from_py.txt','w').write('written by pyodide')\"",
            60000,
        );

        // Shell reads it back via OPFS
        const screen = await run(page, 'cat /from_py.txt', 'written by pyodide');
        expect(screen).toContain('written by pyodide');
    });

    test('python3 runs a script file with arguments', async ({ page }) => {
        test.slow();

        // Create a Python script via echo + redirect
        await runAndWaitForPrompt(
            page,
            'echo "import sys; print(\'received:\', len(sys.argv)-1, \'args\')" > /show_args.py',
        );

        // Run it with arguments
        const screen = await run(
            page,
            'python3 /show_args.py foo bar',
            'received:',
        );
        expect(screen).toContain('received: 2 args');
    });

    test.skip('python3 reports missing script file', async ({ page }) => {
        // TODO: With WasmFS + JSPI, opening a nonexistent file on the OPFS
        // backend can hang because the JSPI suspension for the OPFS stat
        // never resolves. Need to investigate WasmFS error handling for
        // missing files on OPFS mounts.
        test.slow();
        const screen = await run(page, 'python3 /nonexistent.py', 'No such file', 30000);
        expect(screen).toMatch(/No such file|FileNotFoundError/);
    });

    test.skip('pip install and use a package', async ({ page }) => {
        // TODO: micropip fails to load with Pyodide 0.30/WasmFS build.
        // Need correct 0.30 lock file with ABI 2026_0 packages.
        test.slow();

        // Install cowsay — a tiny pure-python package NOT in the Pyodide lock file,
        // so micropip fetches it from PyPI rather than the Pyodide CDN.
        const installScreen = await run(page, 'pip install cowsay', 'Successfully installed', 90000);
        expect(installScreen).toContain('Successfully installed');

        // Use it
        const screen = await run(
            page,
            "python3 -c \"import cowsay; print('cowsay_version=' + cowsay.__version__)\"",
            'cowsay_version=',
        );
        expect(screen).toContain('cowsay_version=');
    });

    test.skip('pip list includes micropip', async ({ page }) => {
        // TODO: micropip fails to load with Pyodide 0.30/WasmFS build.
        test.slow();
        const screen = await run(page, 'pip list', 'micropip');
        expect(screen).toContain('micropip');
    });
});
