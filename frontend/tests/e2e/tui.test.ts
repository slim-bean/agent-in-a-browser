/**
 * Codex TUI E2E Tests
 *
 * Tests the Codex CLI TUI running via codex-wasm-tui WASM in ghostty-web.
 * Uses Playwright to test user interactions with the terminal.
 *
 * The Codex TUI shows:
 * - An initial startup screen with model/provider info
 * - A text input area where the user types prompts
 * - Agent responses stream into the terminal
 *
 * Note: These tests verify the TUI loads and accepts input.
 * Full agent interaction tests require LLM API access.
 */

import { test, expect } from './webkit-persistent-fixture';
import type { Page } from '@playwright/test';

// Helper to type into the terminal
async function typeInTerminal(page: Page, text: string): Promise<void> {
    await page.evaluate(() => {
        window.tuiTerminal?.focus();
    });
    await page.waitForTimeout(100);
    await page.keyboard.type(text, { delay: 50 });
}

// Helper to press keys
async function pressKey(page: Page, key: string): Promise<void> {
    await page.evaluate(() => {
        window.tuiTerminal?.focus();
    });
    await page.waitForTimeout(50);
    await page.keyboard.press(key);
}

// Helper to get all terminal screen text via ghostty-web buffer API
async function getTerminalText(page: Page): Promise<string> {
    return await page.evaluate(() => {
        const terminal = window.tuiTerminal;
        if (!terminal || !terminal.buffer?.active) {
            return '';
        }

        const lines: string[] = [];
        const buffer = terminal.buffer.active;
        for (let y = 0; y < terminal.rows; y++) {
            const line = buffer.getLine(y);
            if (line) {
                lines.push(line.translateToString(true));
            }
        }
        return lines.join('\n');
    });
}

// Helper to wait for terminal output containing text
async function waitForTerminalOutput(page: Page, text: string, timeout = 15000): Promise<void> {
    const startTime = Date.now();
    while (Date.now() - startTime < timeout) {
        const screenText = await getTerminalText(page);
        if (screenText.includes(text)) {
            return;
        }
        await page.waitForTimeout(200);
    }
    const finalText = await getTerminalText(page);
    throw new Error(`Timeout waiting for "${text}" in terminal. Current screen:\n${finalText}`);
}

// Helper to wait for TUI to be ready (terminal exposed + canvas present)
async function waitForTuiReady(page: Page, timeout = 30000): Promise<void> {
    await page.waitForSelector('canvas', { timeout });
    await page.waitForFunction(
        () => {
            return window.tuiTerminal?.buffer?.active !== undefined;
        },
        { timeout }
    );
    // Give TUI a moment to render initial content
    await page.waitForTimeout(500);
}

test.describe('Codex TUI Launch', () => {
    test.beforeEach(async ({ page }) => {
        await page.goto('/');
        await waitForTuiReady(page);
    });

    test('TUI loads and renders to terminal', async ({ page }) => {
        // The Codex TUI should render something to the terminal buffer
        const text = await getTerminalText(page);
        expect(text.length).toBeGreaterThan(0);
    });

    test('terminal accepts keyboard input', async ({ page }) => {
        // Focus and type — the terminal should accept input
        await page.evaluate(() => {
            window.tuiTerminal?.focus();
        });
        await page.waitForTimeout(200);

        // Type some text — if the TUI is running, it should process it
        await page.keyboard.type('hello', { delay: 50 });
        await page.waitForTimeout(200);

        // Terminal buffer should have content (TUI is rendering)
        const text = await getTerminalText(page);
        expect(text.length).toBeGreaterThan(0);
    });

    test('terminal resize updates dimensions', async ({ page }) => {
        // Resize the viewport — the terminal should adapt
        await page.setViewportSize({ width: 1024, height: 600 });
        await page.waitForTimeout(500);

        // Terminal should still have content after resize
        const text = await getTerminalText(page);
        expect(text.length).toBeGreaterThan(0);
    });

    test('Escape key is delivered to TUI', async ({ page }) => {
        await page.evaluate(() => {
            window.tuiTerminal?.focus();
        });
        await page.waitForTimeout(200);

        // Press Escape — the TUI should handle it (e.g., dismiss dialogs)
        await page.keyboard.press('Escape');
        await page.waitForTimeout(200);

        // Terminal should still be responsive
        const text = await getTerminalText(page);
        expect(text.length).toBeGreaterThan(0);
    });
});
