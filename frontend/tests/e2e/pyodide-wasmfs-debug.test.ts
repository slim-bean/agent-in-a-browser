/**
 * Pyodide WasmFS Debug Test
 *
 * Focused test for debugging the WasmFS + JSPI + OPFS Pyodide build.
 * Captures all console output and reports detailed status.
 */

import { test, expect } from '@playwright/test';

test.describe('Pyodide WasmFS Debug', () => {
    test('load pyodide and check filesystem', async ({ page }) => {
        test.setTimeout(120000);

        const logs: string[] = [];
        const errors: string[] = [];

        page.on('console', msg => {
            const text = msg.text();
            if (msg.type() === 'error') {
                errors.push(text);
                console.log('[ERROR]', text);
            } else if (msg.type() === 'warning') {
                console.log('[WARN]', text);
                logs.push('[WARN] ' + text);
            } else {
                // Only log pyodide/loader/wasmfs related messages
                if (text.includes('Pyodide') || text.includes('opfs') || text.includes('OPFS') ||
                    text.includes('WasmFS') || text.includes('wasmfs') || text.includes('PyodideLoader') ||
                    text.includes('Aborted') || text.includes('promising') || text.includes('Suspend') ||
                    text.includes('main') || text.includes('stdlib') || text.includes('encodings') ||
                    text.includes('callMain') || text.includes('run_main') ||
                    text.includes('[LazyProcess]') || text.includes('[PyodideModule]')) {
                    console.log('[LOG]', text);
                }
                logs.push(text);
            }
        });

        page.on('pageerror', err => {
            console.log('[PAGE ERROR]', err.message);
            errors.push(err.message);
        });

        await page.goto('/');

        // Wait for shell prompt
        console.log('Waiting for shell to load...');
        await page.waitForSelector('canvas', { timeout: 30000 });
        await page.waitForFunction(
            () => (window as any).tuiTerminal?.buffer?.active !== undefined,
            { timeout: 30000 },
        );

        // Wait for shell prompt (/$)
        const promptRe = /\/.*\$$/m;
        const start = Date.now();
        while (Date.now() - start < 30000) {
            const screen = await page.evaluate(() => {
                const t = (window as any).tuiTerminal;
                if (!t?.buffer?.active) return '';
                const lines: string[] = [];
                for (let y = 0; y < t.rows; y++) {
                    const line = t.buffer.active.getLine(y);
                    if (line) lines.push(line.translateToString(true));
                }
                return lines.join('\n');
            });
            if (promptRe.test(screen)) break;
            await page.waitForTimeout(250);
        }
        console.log('Shell prompt detected');

        // Type the python command
        await page.evaluate(() => { (window as any).tuiTerminal?.focus(); });
        await page.waitForTimeout(100);

        const cmd = "python3 -c \"import os; print('root:', os.listdir('/')); print('home_user:', os.listdir('/home/user')); print('cwd:', os.getcwd())\"";
        await page.keyboard.type(cmd, { delay: 10 });
        await page.keyboard.press('Enter');

        console.log('Command sent, waiting for output...');

        // Wait up to 90s for Pyodide to load and produce output
        let finalScreen = '';
        const cmdStart = Date.now();
        while (Date.now() - cmdStart < 90000) {
            finalScreen = await page.evaluate(() => {
                const t = (window as any).tuiTerminal;
                if (!t?.buffer?.active) return '';
                const lines: string[] = [];
                for (let y = 0; y < t.rows; y++) {
                    const line = t.buffer.active.getLine(y);
                    if (line) lines.push(line.translateToString(true));
                }
                return lines.join('\n');
            });

            // Find output AFTER the command line
            const allLines = finalScreen.split('\n');
            const cmdIdx = allLines.findIndex(l => l.includes('python3 -c'));
            if (cmdIdx >= 0) {
                const output = allLines.slice(cmdIdx + 1).join('\n').trim();
                // Check for real output (not just blank lines)
                if (output.length > 0 && (
                    output.includes('root:') || output.includes('/opfs') ||
                    output.includes('Traceback') || output.includes('Error') ||
                    output.includes('Aborted') || output.includes('Python') ||
                    promptRe.test(output)
                )) {
                    console.log(`Output detected after ${((Date.now() - cmdStart) / 1000).toFixed(1)}s`);
                    break;
                }
            }

            await page.waitForTimeout(1000);
        }
        console.log(`Total wait: ${((Date.now() - cmdStart) / 1000).toFixed(1)}s`);

        console.log('\n=== TERMINAL OUTPUT ===');
        const lines = finalScreen.split('\n').filter(l => l.trim());
        for (const line of lines.slice(-20)) {
            console.log(line);
        }

        console.log('\n=== ERRORS ===');
        for (const err of errors.slice(-10)) {
            console.log(err.slice(0, 200));
        }

        console.log('\n=== PYODIDE-RELATED LOGS ===');
        const pyLogs = logs.filter(l =>
            l.includes('Pyodide') || l.includes('OPFS') || l.includes('opfs') ||
            l.includes('stdlib') || l.includes('callMain') || l.includes('run_main') ||
            l.includes('Aborted') || l.includes('memory access') || l.includes('Suspend') ||
            l.includes('promising') || l.includes('exitCode') || l.includes('encodings') ||
            l.includes('[WARN]')
        );
        for (const log of pyLogs.slice(-20)) {
            console.log(log.slice(0, 200));
        }

        // Assert Python booted — look for output AFTER the command line
        const cmdLineIdx = finalScreen.split('\n').findIndex(l => l.includes('python3 -c'));
        const outputLines = cmdLineIdx >= 0
            ? finalScreen.split('\n').slice(cmdLineIdx + 1).join('\n')
            : '';
        console.log('\n=== OUTPUT AFTER COMMAND ===');
        console.log(outputLines || '(no output yet)');

        // The test passes if Python produced output with 'root:' or '/opfs'
        // If it fails, the console logs above show what went wrong
        expect(outputLines).toContain('home_user:');
    });
});
