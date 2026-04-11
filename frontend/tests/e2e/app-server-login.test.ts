/**
 * App-Server Login Flow E2E Tests
 *
 * Tests the app-server page boot sequence and login flow.
 * Requires: vite build + copy-externals (dist/ must have wasi-shims/ and wasm-loader/)
 */

import { test, expect, type Page, type ConsoleMessage } from '@playwright/test';

// Helper: collect console messages
function collectConsole(page: Page): ConsoleMessage[] {
    const logs: ConsoleMessage[] = [];
    page.on('console', (msg) => logs.push(msg));
    return logs;
}

function hasLog(logs: ConsoleMessage[], text: string): boolean {
    return logs.some((m) => m.text().includes(text));
}

// Helper: wait for WASM boot to complete
async function waitForBoot(page: Page, logs: ConsoleMessage[]): Promise<void> {
    await expect(async () => {
        expect(hasLog(logs, 'WASM runtime started')).toBe(true);
    }).toPass({ timeout: 30_000 });
}

test.describe('App Server Boot', () => {
    test('WASM worker starts without createSyncAccessHandle errors', async ({ page }) => {
        const logs = collectConsole(page);
        await page.goto('/app-server');
        await waitForBoot(page, logs);

        // Verify no sync handle errors (the whole point of the Worker refactor)
        const syncHandleError = logs.find((m) =>
            m.text().includes('createSyncAccessHandle is not a function')
        );
        expect(syncHandleError).toBeUndefined();

        // Verify boot stages
        expect(hasLog(logs, 'Sandbox ready')).toBe(true);
        expect(hasLog(logs, 'Worker ready')).toBe(true);
        expect(hasLog(logs, 'app-server started successfully')).toBe(true);
    });

    test('login screen appears when not authenticated', async ({ page }) => {
        const logs = collectConsole(page);
        await page.goto('/app-server');
        await waitForBoot(page, logs);

        // Login screen visible
        await expect(page.locator('.login-screen')).toBeVisible({ timeout: 5_000 });
        await expect(page.locator('.login-screen__title')).toHaveText('Edge Agent');

        // API key input present
        await expect(page.locator('.login-screen__input[type="password"]')).toBeVisible();

        // Device code button present
        await expect(page.locator('button', { hasText: 'Sign in with OpenAI' })).toBeVisible();

        // Chat UI should be hidden
        await expect(page.locator('.messages')).not.toBeVisible();
    });
});

test.describe('Device Code Login', () => {
    test('clicking Sign in with OpenAI requests device code', async ({ page }) => {
        const logs = collectConsole(page);
        await page.goto('/app-server');
        await waitForBoot(page, logs);

        // Wait for login screen
        await expect(page.locator('button', { hasText: 'Sign in with OpenAI' })).toBeVisible({ timeout: 5_000 });

        // Click device code button
        await page.locator('button', { hasText: 'Sign in with OpenAI' }).click();

        // Monitor ALL network requests to see if CORS proxy is used
        page.on('request', (req) => {
            const url = req.url();
            if (url.includes('cors-proxy') || url.includes('openai') || url.includes('auth')) {
                console.log(`[NETWORK] ${req.method()} ${url}`);
            }
        });
        page.on('response', (resp) => {
            const url = resp.url();
            if (url.includes('cors-proxy') || url.includes('openai') || url.includes('auth')) {
                console.log(`[NETWORK RESP] ${resp.status()} ${url}`);
            }
        });

        // Should show "Requesting device code..." status
        await expect(page.locator('.login-screen__status')).toBeVisible({ timeout: 2_000 });

        // Wait for device code display or error (up to 20s for network)
        const deviceCode = page.locator('.login-screen__user-code');
        const loginError = page.locator('.login-screen__error');

        await expect(async () => {
            const codeVisible = await deviceCode.isVisible().catch(() => false);
            const errorVisible = await loginError.isVisible().catch(() => false);

            // Log HTTP-related console messages for debugging
            const httpLogs = logs.filter((m) =>
                m.text().includes('http') || m.text().includes('auth') ||
                m.text().includes('device') || m.text().includes('transport') ||
                m.text().includes('cors') || m.text().includes('proxy') ||
                m.text().includes('openai') || m.text().includes('chatgpt')
            );
            if (httpLogs.length > 0) {
                console.log(`[device-code poll] code=${codeVisible} error=${errorVisible} httpLogs=${httpLogs.length}`);
                for (const msg of httpLogs.slice(-5)) {
                    console.log(`  [${msg.type()}] ${msg.text()}`);
                }
            }

            expect(codeVisible || errorVisible).toBe(true);
        }).toPass({ timeout: 20_000 });

        if (await deviceCode.isVisible()) {
            // Device code appeared — verify the display
            const code = await deviceCode.textContent();
            expect(code).toBeTruthy();
            expect(code!.length).toBeGreaterThan(0);
            console.log(`Device code received: ${code}`);

            // Verification URL should be a link
            const urlLink = page.locator('.login-screen__device-url');
            await expect(urlLink).toBeVisible();
            const href = await urlLink.getAttribute('href');
            expect(href).toContain('http');
            console.log(`Verification URL: ${href}`);

            // Copy and Cancel buttons should exist
            await expect(page.locator('button', { hasText: 'Copy Code' })).toBeVisible();
            await expect(page.locator('button', { hasText: 'Cancel' })).toBeVisible();

            // Cancel should restore the login form
            await page.locator('button', { hasText: 'Cancel' }).click();
            await expect(page.locator('.login-screen__input[type="password"]')).toBeVisible();
            await expect(deviceCode).not.toBeVisible();
        } else {
            // Error case — log it but still pass (auth endpoint may reject)
            const errorText = await loginError.textContent();
            console.log(`Device code error (expected if no auth backend): ${errorText}`);

            // Dump relevant logs
            for (const msg of logs) {
                if (msg.text().includes('http') || msg.text().includes('auth') || msg.text().includes('device')) {
                    console.log(`  [${msg.type()}] ${msg.text()}`);
                }
            }
        }
    });
});

test.describe('API Key Login', () => {
    test('submitting API key triggers login request', async ({ page }) => {
        const logs = collectConsole(page);
        await page.goto('/app-server');
        await waitForBoot(page, logs);

        // Wait for login screen
        const apiKeyInput = page.locator('.login-screen__input[type="password"]');
        await expect(apiKeyInput).toBeVisible({ timeout: 5_000 });

        // Type API key
        await apiKeyInput.fill('sk-test-fake-key-for-e2e-testing');

        // Submit
        await page.locator('button', { hasText: 'Sign in with API Key' }).click();

        // Should show status or transition
        // With a fake key, the app-server will save it and potentially show chat
        // (API key login succeeds immediately since there's no validation at login time)
        await expect(async () => {
            const chatVisible = await page.locator('.messages').isVisible().catch(() => false);
            const errorVisible = await page.locator('.login-screen__error').isVisible().catch(() => false);
            const statusText = await page.locator('.login-screen__status').textContent().catch(() => '');
            expect(chatVisible || errorVisible || (statusText !== null && statusText !== '')).toBe(true);
        }).toPass({ timeout: 15_000 });

        // Log what happened for debugging
        const chatVisible = await page.locator('.messages').isVisible().catch(() => false);
        if (chatVisible) {
            console.log('API key login succeeded — chat UI visible');
            // Login screen should be gone
            await expect(page.locator('.login-screen')).not.toBeVisible();
        } else {
            const errorText = await page.locator('.login-screen__error').textContent().catch(() => null);
            console.log(`API key login result: error="${errorText}"`);
        }
    });
});
