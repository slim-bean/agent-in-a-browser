/**
 * Debug script: monitors TUI startup progress without breaking JSPI.
 * Run with: node tests/debug-tui-wasi-trace.mjs
 */

import { chromium } from '../../node_modules/.pnpm/playwright@1.57.0/node_modules/playwright/index.mjs';

const BASE_URL = process.env.BASE_URL || 'http://localhost:8081';

async function main() {
    const browser = await chromium.launch({
        headless: true,
        args: [
            '--enable-experimental-web-platform-features',
            '--enable-features=WebAssemblyJSPromiseIntegration',
        ],
    });

    const page = await browser.newPage();
    const logs = [];

    page.on('console', msg => {
        const text = msg.text();
        logs.push({ time: Date.now(), text });
        // Print key messages in real-time
        if (text.includes('tui-trace') || text.includes('codex-wasm-tui') ||
            text.includes('TUI error') || text.includes('TUI Loader') ||
            text.includes('TUI running') || text.includes('WASM stderr')) {
            console.log(text);
        }
    });

    page.on('pageerror', err => {
        console.log(`[PAGE ERROR] ${err.message}`);
    });

    console.log(`Navigating to ${BASE_URL}...`);
    await page.goto(BASE_URL);

    // Monitor for 60 seconds
    for (let i = 0; i < 60; i++) {
        await new Promise(r => setTimeout(r, 1000));

        try {
            const result = await page.evaluate({ timeout: 800 }, () => {
                const t = window.tuiTerminal;
                if (!t?.buffer?.active) return { status: 'no-buffer' };
                const lines = [];
                for (let y = 0; y < Math.min(t.rows, 10); y++) {
                    const line = t.buffer.active.getLine(y);
                    if (line) lines.push(line.translateToString(true));
                }
                const nonEmpty = lines.filter(l => l.trim());
                return {
                    status: 'ok',
                    rows: nonEmpty.length,
                    text: nonEmpty.join('\n'),
                };
            });

            if (result.status === 'ok' && result.rows > 0 &&
                !result.text.includes('Loading Codex')) {
                console.log(`\n=== TUI RENDERED at ${i+1}s ===`);
                console.log(result.text);
                break;
            }
            // Page is responsive but TUI hasn't rendered yet
            if (i % 5 === 4) console.log(`${i+1}s: page responsive, waiting for TUI...`);
        } catch {
            if (i % 5 === 0) console.log(`${i+1}s: page blocked`);
        }
    }

    // Print all stderr messages
    const stderr = logs.filter(l => l.text.includes('WASM stderr') || l.text.includes('tui-trace'));
    if (stderr.length) {
        console.log(`\n=== All stderr/trace messages (${stderr.length}) ===`);
        stderr.forEach(l => console.log(l.text));
    }

    await browser.close();
}

main().catch(e => { console.error(e); process.exit(1); });
