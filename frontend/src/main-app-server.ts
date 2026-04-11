/**
 * Main entry point for the App Server chat UI.
 *
 * Production frontend for the browser-based AI coding agent. Renders all
 * protocol event types with dedicated item renderers, streaming support,
 * inline approval flows, and smooth auto-scrolling.
 */

import { launchAppServer, type ApprovalDecision } from './wasm/app-server/app-server-loader.js';
import {
    AppServerClient,
    type AgentMessageDelta,
    type ItemStartedEvent,
    type ItemCompletedEvent,
    type TurnCompletedEvent,
    type TurnStartedEvent,
    type CommandApprovalRequest,
    type FileChangeApprovalRequest,
    type PermissionsApprovalRequest,
    type ToolUserInputRequest,
    type PatchApprovalRequest,
    type ThreadStartResponse,
    type ThreadItem,
    type CommandExecutionItem,
    type FileChangeItem,
    type FileUpdateChange,
    type McpToolCallItem,
    type DynamicToolCallItem,
    type ReasoningItem,
    type PlanItem,
    type WebSearchItem,
    type AgentMessageItem,
    type TokenUsage,
    type CommandExecutionOutputDeltaNotification,
    type FileChangeOutputDeltaNotification,
    type PlanDeltaNotification,
    type ReasoningTextDeltaNotification,
    type ReasoningSummaryTextDeltaNotification,
    type McpToolCallProgressNotification,
    type ThreadTokenUsageUpdatedNotification,
    type ThreadStatusChangedNotification,
    type ErrorNotification,
    type HookStartedNotification,
    type HookCompletedNotification,
    type GuardianReviewStartedNotification,
    type GuardianReviewCompletedNotification,
    type LoginAccountResponse,
    type AccountLoginCompletedNotification,
    type AccountUpdatedNotification,
    type Account,
    type GetAccountResponse,
} from './wasm/app-server/protocol-client.js';
import './app-server.css';

// ==========================================================================
// Markdown Renderer
// ==========================================================================

/** Escape HTML entities in user content to prevent XSS. */
function escapeHtml(text: string): string {
    return text
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;')
        .replace(/'/g, '&#39;');
}

/**
 * Lightweight markdown-to-HTML renderer. No external dependencies.
 * Sanitizes input by escaping HTML before processing markdown syntax.
 */
function renderMarkdown(text: string): string {
    // Extract code blocks first to protect them from other transforms
    const codeBlocks: string[] = [];
    let processed = text.replace(/```(\w*)\n([\s\S]*?)```/g, (_match, lang: string, code: string) => {
        const langAttr = lang ? ` class="language-${escapeHtml(lang)}"` : '';
        const idx = codeBlocks.length;
        codeBlocks.push(`<pre><code${langAttr}>${escapeHtml(code.replace(/\n$/, ''))}</code></pre>`);
        return `\x00CODEBLOCK${idx}\x00`;
    });

    // Escape HTML in the remaining text (not inside code blocks)
    processed = escapeHtml(processed);

    // Restore code blocks (they were already escaped internally)
    processed = processed.replace(/\x00CODEBLOCK(\d+)\x00/g, (_match, idx: string) => {
        return codeBlocks[parseInt(idx, 10)];
    });

    // Horizontal rules (must be before list processing)
    processed = processed.replace(/^---+$/gm, '<hr>');

    // Headings
    processed = processed.replace(/^#### (.+)$/gm, '<h4>$1</h4>');
    processed = processed.replace(/^### (.+)$/gm, '<h3>$1</h3>');
    processed = processed.replace(/^## (.+)$/gm, '<h2>$1</h2>');
    processed = processed.replace(/^# (.+)$/gm, '<h1>$1</h1>');

    // Blockquotes
    processed = processed.replace(/^&gt; (.+)$/gm, '<blockquote>$1</blockquote>');

    // Unordered lists
    processed = processed.replace(/^(?:[*-]) (.+)$/gm, '<li>$1</li>');
    processed = processed.replace(/((?:<li>.*<\/li>\n?)+)/g, '<ul>$1</ul>');

    // Ordered lists
    processed = processed.replace(/^\d+\. (.+)$/gm, '<li>$1</li>');
    // Wrap consecutive <li> not already in <ul> into <ol>
    processed = processed.replace(/<\/ul>\s*<ul>/g, ''); // merge adjacent ul
    processed = processed.replace(
        /(?<!<\/ul>)((?:<li>.*<\/li>\n?)+)(?!<\/ul>)/g,
        (_match, items: string) => {
            // Check if already wrapped
            return `<ol>${items}</ol>`;
        },
    );

    // Inline code (protect from bold/italic processing)
    const inlineCodes: string[] = [];
    processed = processed.replace(/`([^`]+)`/g, (_match, code: string) => {
        const idx = inlineCodes.length;
        inlineCodes.push(`<code>${code}</code>`);
        return `\x00INLINE${idx}\x00`;
    });

    // Bold
    processed = processed.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');

    // Italic (careful not to match inside words for underscore)
    processed = processed.replace(/\*(.+?)\*/g, '<em>$1</em>');
    processed = processed.replace(/(?<!\w)_(.+?)_(?!\w)/g, '<em>$1</em>');

    // Links
    processed = processed.replace(
        /\[([^\]]+)\]\(([^)]+)\)/g,
        '<a href="$2" target="_blank" rel="noopener">$1</a>',
    );

    // Restore inline code
    processed = processed.replace(/\x00INLINE(\d+)\x00/g, (_match, idx: string) => {
        return inlineCodes[parseInt(idx, 10)];
    });

    // Paragraphs: split on double newlines
    const blocks = processed.split(/\n\n+/);
    processed = blocks
        .map((block) => {
            const trimmed = block.trim();
            if (!trimmed) return '';
            // Don't wrap block-level elements in <p>
            if (
                trimmed.startsWith('<h') ||
                trimmed.startsWith('<pre>') ||
                trimmed.startsWith('<ul') ||
                trimmed.startsWith('<ol') ||
                trimmed.startsWith('<blockquote') ||
                trimmed.startsWith('<hr')
            ) {
                return trimmed;
            }
            // Convert single newlines to <br> within paragraphs
            return `<p>${trimmed.replace(/\n/g, '<br>')}</p>`;
        })
        .join('\n');

    return processed;
}

// ==========================================================================
// State
// ==========================================================================

interface TrackedItem {
    element: HTMLElement;
    streamBuffer: string;
}

interface AppState {
    client: AppServerClient | null;
    threadId: string | null;
    turnId: string | null;
    turnActive: boolean;
    tokenUsage: TokenUsage | null;
    bootStage: string;
    items: Map<string, TrackedItem>;
}

const state: AppState = {
    client: null,
    threadId: null,
    turnId: null,
    turnActive: false,
    tokenUsage: null,
    bootStage: 'Initializing...',
    items: new Map(),
};

// ==========================================================================
// DOM References (populated in buildUI)
// ==========================================================================

let statusDot: HTMLElement;
let statusText: HTMLElement;
let statusCenter: HTMLElement;
let statusRight: HTMLElement;
let messagesContainer: HTMLElement;
let thinkingIndicator: HTMLElement;
let scrollAnchor: HTMLElement;
let inputArea: HTMLElement;
let inputTextarea: HTMLTextAreaElement;
let inputForm: HTMLFormElement;
let sendButton: HTMLButtonElement;
let inputHint: HTMLElement;
let statusAccount: HTMLElement;
let statusTokens: HTMLElement;

// Auth state
let authenticated = false;
let currentAccount: Account | null = null;
let loginScreenEl: HTMLElement | null = null;
let pendingLoginId: string | null = null;

// Tracks current streaming assistant message
let currentAssistantContent: HTMLElement | null = null;
let currentAssistantBuffer = '';

// Debounce timer for assistant message rendering
let assistantRenderTimer: ReturnType<typeof setTimeout> | null = null;
let assistantDeltaCount = 0;
const ASSISTANT_RENDER_INTERVAL = 50;
const ASSISTANT_RENDER_DELTA_THRESHOLD = 3;

// ==========================================================================
// Utility: DOM helpers
// ==========================================================================

function el(tag: string, ...classNames: string[]): HTMLElement {
    const element = document.createElement(tag);
    for (const cn of classNames) {
        element.classList.add(cn);
    }
    return element;
}

/** Format milliseconds as human-readable duration. */
function formatDuration(ms: number): string {
    if (ms < 1000) return `${ms}ms`;
    const seconds = ms / 1000;
    if (seconds < 60) return `${seconds.toFixed(1)}s`;
    const minutes = Math.floor(seconds / 60);
    const remaining = seconds % 60;
    return `${minutes}m ${remaining.toFixed(0)}s`;
}

/** Format token count with commas. */
function formatTokens(n: number): string {
    return n.toLocaleString();
}

// ==========================================================================
// Scrolling
// ==========================================================================

function isNearBottom(): boolean {
    const threshold = 100;
    const { scrollTop, scrollHeight, clientHeight } = messagesContainer;
    return scrollHeight - scrollTop - clientHeight < threshold;
}

function scrollToBottom(smooth = true): void {
    messagesContainer.scrollTo({
        top: messagesContainer.scrollHeight,
        behavior: smooth ? 'smooth' : 'auto',
    });
}

function updateScrollAnchor(): void {
    if (isNearBottom()) {
        scrollAnchor.classList.remove('scroll-anchor--visible');
        scrollAnchor.classList.add('scroll-anchor--hidden');
    } else {
        scrollAnchor.classList.remove('scroll-anchor--hidden');
        scrollAnchor.classList.add('scroll-anchor--visible');
    }
}

function autoScroll(): void {
    if (isNearBottom()) {
        scrollToBottom(false);
    }
    updateScrollAnchor();
}

// ==========================================================================
// Status Bar
// ==========================================================================

function setStatus(status: 'booting' | 'connected' | 'thinking' | 'error', label?: string): void {
    statusDot.classList.remove(
        'status-bar__dot--connected',
        'status-bar__dot--thinking',
        'status-bar__dot--error',
    );

    switch (status) {
        case 'booting':
            statusText.textContent = label ?? 'Booting...';
            break;
        case 'connected':
            statusDot.classList.add('status-bar__dot--connected');
            statusText.textContent = label ?? 'Connected';
            break;
        case 'thinking':
            statusDot.classList.add('status-bar__dot--thinking');
            statusText.textContent = label ?? 'Thinking...';
            break;
        case 'error':
            statusDot.classList.add('status-bar__dot--error');
            statusText.textContent = label ?? 'Error';
            break;
    }
}

function updateTokenUsage(usage: TokenUsage): void {
    state.tokenUsage = usage;
    const parts: string[] = [];
    parts.push(`In: ${formatTokens(usage.total.inputTokens)}`);
    parts.push(`Out: ${formatTokens(usage.total.outputTokens)}`);
    if (usage.total.cachedInputTokens > 0) {
        parts.push(`Cached: ${formatTokens(usage.total.cachedInputTokens)}`);
    }
    statusTokens.textContent = parts.join(' | ');
}

function updateThreadInfo(): void {
    if (state.threadId) {
        statusCenter.textContent = `Thread: ${state.threadId.slice(0, 8)}...`;
    } else {
        statusCenter.textContent = '';
    }
}

// ==========================================================================
// Thinking Indicator
// ==========================================================================

function showThinking(): void {
    thinkingIndicator.classList.remove('thinking--hidden');
    autoScroll();
}

function hideThinking(): void {
    thinkingIndicator.classList.add('thinking--hidden');
}

// ==========================================================================
// UI Construction
// ==========================================================================

function buildUI(): void {
    const app = document.getElementById('app');
    if (!app) {
        throw new Error('Missing #app element in document');
    }
    app.classList.add('app');

    // --- Status Bar ---
    const statusBarEl = el('div', 'status-bar');

    const leftSection = el('div', 'status-bar__section');
    statusDot = el('span', 'status-bar__dot');
    statusText = el('span', 'status-bar__item');
    statusText.textContent = 'Booting...';
    leftSection.appendChild(statusDot);
    leftSection.appendChild(statusText);

    statusCenter = el('div', 'status-bar__section');
    statusRight = el('div', 'status-bar__section');

    statusAccount = el('div', 'status-bar__account');
    statusTokens = el('div', 'status-bar__item');
    statusRight.appendChild(statusAccount);
    statusRight.appendChild(statusTokens);

    statusBarEl.appendChild(leftSection);
    statusBarEl.appendChild(statusCenter);
    statusBarEl.appendChild(statusRight);
    app.appendChild(statusBarEl);

    // --- Messages Area ---
    messagesContainer = el('div', 'messages');
    messagesContainer.addEventListener('scroll', updateScrollAnchor);
    app.appendChild(messagesContainer);

    // --- Thinking Indicator (inside messages, hidden initially) ---
    thinkingIndicator = el('div', 'thinking', 'thinking--hidden');
    const dotsContainer = el('div', 'thinking__dots');
    for (let i = 0; i < 3; i++) {
        dotsContainer.appendChild(el('span', 'thinking__dot'));
    }
    thinkingIndicator.appendChild(dotsContainer);
    const thinkingText = el('span', 'thinking__text');
    thinkingText.textContent = 'Agent is thinking...';
    thinkingIndicator.appendChild(thinkingText);
    messagesContainer.appendChild(thinkingIndicator);

    // --- Scroll Anchor ---
    scrollAnchor = el('div', 'scroll-anchor', 'scroll-anchor--hidden');
    scrollAnchor.textContent = '\u2193 New messages';
    scrollAnchor.setAttribute('role', 'button');
    scrollAnchor.setAttribute('aria-label', 'Scroll to bottom');
    scrollAnchor.addEventListener('click', () => scrollToBottom(true));
    app.appendChild(scrollAnchor);

    // --- Input Area ---
    inputArea = el('div', 'input-area');

    inputForm = document.createElement('form');
    inputForm.classList.add('input-area__form');
    inputForm.addEventListener('submit', (e) => void handleSubmit(e));

    inputTextarea = document.createElement('textarea');
    inputTextarea.classList.add('input-area__textarea');
    inputTextarea.rows = 1;
    inputTextarea.placeholder = 'Loading...';
    inputTextarea.disabled = true;
    inputTextarea.setAttribute('aria-label', 'Message input');
    inputTextarea.addEventListener('input', handleTextareaAutoGrow);
    inputTextarea.addEventListener('keydown', handleTextareaKeydown);

    const actions = el('div', 'input-area__actions');

    inputHint = el('span', 'input-area__hint');
    inputHint.textContent = 'Enter to send \u00b7 Shift+Enter for newline \u00b7 Ctrl+C to interrupt';

    sendButton = document.createElement('button');
    sendButton.classList.add('input-area__btn');
    sendButton.type = 'submit';
    sendButton.textContent = 'Send';
    sendButton.setAttribute('aria-label', 'Send message');

    actions.appendChild(inputHint);
    actions.appendChild(sendButton);

    inputForm.appendChild(inputTextarea);
    inputForm.appendChild(actions);
    inputArea.appendChild(inputForm);
    app.appendChild(inputArea);
}

// ==========================================================================
// Login Screen
// ==========================================================================

function enableLoginButtons(): void {
    if (!loginScreenEl) return;
    const buttons = loginScreenEl.querySelectorAll('button');
    for (const btn of buttons) {
        (btn as HTMLButtonElement).disabled = false;
    }
    const input = loginScreenEl.querySelector('.login-screen__input') as HTMLInputElement | null;
    if (input) input.disabled = false;
    const statusEl = loginScreenEl.querySelector('.login-screen__status') as HTMLElement | null;
    if (statusEl) statusEl.classList.remove('login-screen__status--visible');
}

function disableLoginButtons(): void {
    if (!loginScreenEl) return;
    const buttons = loginScreenEl.querySelectorAll('button');
    for (const btn of buttons) {
        (btn as HTMLButtonElement).disabled = true;
    }
    const input = loginScreenEl.querySelector('.login-screen__input') as HTMLInputElement | null;
    if (input) input.disabled = true;
}

function showLoginError(message: string): void {
    if (!loginScreenEl) return;
    const errorEl = loginScreenEl.querySelector('.login-screen__error') as HTMLElement | null;
    if (errorEl) {
        errorEl.textContent = message;
        errorEl.classList.add('login-screen__error--visible');
    }
    enableLoginButtons();
}

function showLoginStatus(message: string): void {
    if (!loginScreenEl) return;
    const statusEl = loginScreenEl.querySelector('.login-screen__status') as HTMLElement | null;
    if (statusEl) {
        statusEl.textContent = message;
        statusEl.classList.add('login-screen__status--visible');
    }
    const errorEl = loginScreenEl.querySelector('.login-screen__error') as HTMLElement | null;
    if (errorEl) errorEl.classList.remove('login-screen__error--visible');
}

function showDeviceCode(verificationUrl: string, userCode: string): void {
    if (!loginScreenEl) return;

    // Hide the form and divider, show device code instructions
    const form = loginScreenEl.querySelector('.login-screen__form') as HTMLElement | null;
    const divider = loginScreenEl.querySelector('.login-screen__divider') as HTMLElement | null;
    const btn = loginScreenEl.querySelector('.login-screen__btn--secondary') as HTMLElement | null;
    if (form) form.style.display = 'none';
    if (divider) divider.style.display = 'none';
    if (btn) btn.style.display = 'none';

    // Remove any previous device code display
    const prev = loginScreenEl.querySelector('.login-screen__device-code');
    if (prev) prev.remove();

    const container = el('div', 'login-screen__device-code');

    const instruction = el('div', 'login-screen__subtitle');
    instruction.textContent = 'Visit the URL below and enter the code to sign in:';
    container.appendChild(instruction);

    const urlLink = document.createElement('a');
    urlLink.classList.add('login-screen__device-url');
    urlLink.href = verificationUrl;
    urlLink.target = '_blank';
    urlLink.rel = 'noopener';
    urlLink.textContent = verificationUrl;
    container.appendChild(urlLink);

    const codeDisplay = el('div', 'login-screen__user-code');
    codeDisplay.textContent = userCode;
    container.appendChild(codeDisplay);

    const copyBtn = document.createElement('button');
    copyBtn.classList.add('login-screen__btn', 'login-screen__btn--secondary');
    copyBtn.type = 'button';
    copyBtn.textContent = 'Copy Code';
    copyBtn.addEventListener('click', () => {
        navigator.clipboard.writeText(userCode).then(() => {
            copyBtn.textContent = 'Copied!';
            setTimeout(() => { copyBtn.textContent = 'Copy Code'; }, 2000);
        }).catch(() => {});
    });
    container.appendChild(copyBtn);

    const waiting = el('div', 'login-screen__status', 'login-screen__status--visible');
    waiting.innerHTML = 'Waiting for authorization<span class="thinking__dots"><span class="thinking__dot"></span><span class="thinking__dot"></span><span class="thinking__dot"></span></span>';
    container.appendChild(waiting);

    const cancelBtn = document.createElement('button');
    cancelBtn.classList.add('login-screen__btn', 'login-screen__btn--secondary');
    cancelBtn.type = 'button';
    cancelBtn.textContent = 'Cancel';
    cancelBtn.addEventListener('click', () => {
        if (state.client && pendingLoginId) {
            state.client.cancelLogin(pendingLoginId).catch(() => {});
        }
        pendingLoginId = null;
        container.remove();
        if (form) form.style.display = '';
        if (divider) divider.style.display = '';
        if (btn) btn.style.display = '';
        enableLoginButtons();
    });
    container.appendChild(cancelBtn);

    loginScreenEl.appendChild(container);
}

function buildLoginScreen(): HTMLElement {
    const screen = el('div', 'login-screen');

    const title = el('div', 'login-screen__title');
    title.textContent = 'Edge Agent';
    screen.appendChild(title);

    const subtitle = el('div', 'login-screen__subtitle');
    subtitle.textContent = 'Sign in to start using the AI coding agent';
    screen.appendChild(subtitle);

    const form = document.createElement('form');
    form.classList.add('login-screen__form');

    const input = document.createElement('input');
    input.classList.add('login-screen__input');
    input.type = 'password';
    input.placeholder = 'OpenAI API Key (sk-...)';
    input.setAttribute('aria-label', 'OpenAI API Key');
    form.appendChild(input);

    const apiKeyBtn = document.createElement('button');
    apiKeyBtn.classList.add('login-screen__btn', 'login-screen__btn--primary');
    apiKeyBtn.type = 'submit';
    apiKeyBtn.textContent = 'Sign in with API Key';
    form.appendChild(apiKeyBtn);

    form.addEventListener('submit', (e) => {
        e.preventDefault();
        const apiKey = input.value.trim();
        if (!apiKey || !state.client) return;

        disableLoginButtons();
        showLoginStatus('Signing in...');

        state.client.loginWithApiKey(apiKey).then((response: LoginAccountResponse) => {
            if (response.type === 'apiKey') {
                // Wait for account/login/completed notification
            }
        }).catch((err: unknown) => {
            const msg = err instanceof Error ? err.message : String(err);
            showLoginError(msg);
        });
    });

    screen.appendChild(form);

    const divider = el('div', 'login-screen__divider');
    divider.textContent = 'or';
    screen.appendChild(divider);

    const deviceCodeBtn = document.createElement('button');
    deviceCodeBtn.classList.add('login-screen__btn', 'login-screen__btn--secondary');
    deviceCodeBtn.type = 'button';
    deviceCodeBtn.textContent = 'Sign in with OpenAI';
    deviceCodeBtn.addEventListener('click', () => {
        if (!state.client) return;

        disableLoginButtons();
        showLoginStatus('Requesting device code...');

        state.client.loginWithDeviceCode().then((response: LoginAccountResponse) => {
            if (response.type === 'chatgptDeviceCode') {
                pendingLoginId = response.loginId;
                showDeviceCode(response.verificationUrl, response.userCode);
            }
        }).catch((err: unknown) => {
            const msg = err instanceof Error ? err.message : String(err);
            enableLoginButtons();
            showLoginError(msg);
        });
    });
    screen.appendChild(deviceCodeBtn);

    const errorEl = el('div', 'login-screen__error');
    screen.appendChild(errorEl);

    const statusEl = el('div', 'login-screen__status');
    screen.appendChild(statusEl);

    return screen;
}

function showLoginScreen(): void {
    if (!loginScreenEl) {
        loginScreenEl = buildLoginScreen();
        const app = document.getElementById('app');
        if (app) {
            // Insert login screen before the input area
            app.insertBefore(loginScreenEl, inputArea);
        }
    }

    messagesContainer.classList.add('messages--hidden');
    inputArea.classList.add('input-area--hidden');
    thinkingIndicator.classList.add('thinking--hidden');
    loginScreenEl.classList.remove('login-screen--hidden');
}

function showChatUI(): void {
    if (loginScreenEl) {
        loginScreenEl.classList.add('login-screen--hidden');
    }

    messagesContainer.classList.remove('messages--hidden');
    inputArea.classList.remove('input-area--hidden');
    setInputEnabled(true);
    setStatus('connected');
}

function updateAccountDisplay(): void {
    statusAccount.innerHTML = '';

    if (!currentAccount) {
        const loginBtn = document.createElement('button');
        loginBtn.classList.add('status-bar__login-btn');
        loginBtn.textContent = 'Sign in';
        loginBtn.addEventListener('click', () => showLoginScreen());
        statusAccount.appendChild(loginBtn);
        return;
    }

    if (currentAccount.type === 'chatgpt') {
        const email = el('span', 'status-bar__account-email');
        email.textContent = currentAccount.email;
        statusAccount.appendChild(email);

        const plan = el('span', 'status-bar__account-plan');
        plan.textContent = currentAccount.planType;
        statusAccount.appendChild(plan);
    } else {
        const label = el('span', 'status-bar__account-email');
        label.textContent = 'API Key';
        statusAccount.appendChild(label);
    }
}

// ==========================================================================
// Input Handling
// ==========================================================================

function setInputEnabled(enabled: boolean): void {
    inputTextarea.disabled = !enabled;
    sendButton.disabled = !enabled;
    inputTextarea.placeholder = enabled ? 'Type a message...' : 'Agent is thinking...';
    if (enabled) {
        inputTextarea.focus();
    }
}

function handleTextareaAutoGrow(): void {
    inputTextarea.style.height = 'auto';
    inputTextarea.style.height = `${inputTextarea.scrollHeight}px`;
}

function handleTextareaKeydown(e: KeyboardEvent): void {
    // Enter sends (unless Shift is held)
    if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        inputForm.requestSubmit();
        return;
    }

    // Ctrl+C / Cmd+C without selection interrupts
    if (e.key === 'c' && (e.ctrlKey || e.metaKey)) {
        const selection = window.getSelection();
        if (!selection || selection.isCollapsed) {
            e.preventDefault();
            void handleInterrupt();
        }
    }
}

async function handleSubmit(e: Event): Promise<void> {
    e.preventDefault();
    console.warn('[App Server] handleSubmit called', { disabled: inputTextarea.disabled, value: inputTextarea.value });
    if (!state.client || inputTextarea.disabled) return;

    const userText = inputTextarea.value.trim();
    if (!userText) return;

    inputTextarea.value = '';
    inputTextarea.style.height = 'auto';
    appendUserMessage(userText);
    setInputEnabled(false);

    try {
        if (!state.threadId) {
            const threadResponse: ThreadStartResponse = await state.client.startThread();
            state.threadId = threadResponse.thread.id;
            updateThreadInfo();
        }

        currentAssistantContent = null;
        currentAssistantBuffer = '';
        assistantDeltaCount = 0;

        await state.client.startTurn(state.threadId, userText);
    } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        appendSystemMessage(`Error: ${msg}`);
        setInputEnabled(true);
    }
}

async function handleInterrupt(): Promise<void> {
    if (!state.client || !state.threadId || !state.turnId || !state.turnActive) return;
    try {
        await state.client.interruptTurn(state.threadId, state.turnId);
    } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        appendSystemMessage(`Interrupt failed: ${msg}`);
    }
}

// ==========================================================================
// Message Rendering
// ==========================================================================

function appendUserMessage(userText: string): void {
    const msg = el('div', 'message', 'message--user');
    const content = el('div', 'message__content');
    content.innerHTML = renderMarkdown(userText);
    msg.appendChild(content);
    messagesContainer.insertBefore(msg, thinkingIndicator);
    autoScroll();
}

function createAssistantMessage(): { element: HTMLElement; content: HTMLElement } {
    const msg = el('div', 'message', 'message--assistant');
    const content = el('div', 'message__content');
    const meta = el('div', 'message__meta');
    msg.appendChild(content);
    msg.appendChild(meta);
    messagesContainer.insertBefore(msg, thinkingIndicator);
    autoScroll();
    return { element: msg, content };
}

function flushAssistantRender(): void {
    if (currentAssistantContent && currentAssistantBuffer) {
        currentAssistantContent.innerHTML = renderMarkdown(currentAssistantBuffer);
        autoScroll();
    }
    assistantDeltaCount = 0;
}

function appendToAssistantMessage(delta: string): void {
    if (!currentAssistantContent) {
        const created = createAssistantMessage();
        currentAssistantContent = created.content;
        currentAssistantBuffer = '';
    }
    currentAssistantBuffer += delta;
    assistantDeltaCount++;

    if (assistantDeltaCount >= ASSISTANT_RENDER_DELTA_THRESHOLD) {
        if (assistantRenderTimer !== null) {
            clearTimeout(assistantRenderTimer);
            assistantRenderTimer = null;
        }
        flushAssistantRender();
    } else if (assistantRenderTimer === null) {
        assistantRenderTimer = setTimeout(() => {
            assistantRenderTimer = null;
            flushAssistantRender();
        }, ASSISTANT_RENDER_INTERVAL);
    }
}

function appendSystemMessage(messageText: string): void {
    const msg = el('div', 'message', 'message--system');
    const content = el('div', 'message__content');
    content.textContent = messageText;
    msg.appendChild(content);
    messagesContainer.insertBefore(msg, thinkingIndicator);
    autoScroll();
}

// ==========================================================================
// Item Renderers
// ==========================================================================

function renderCommandItem(item: CommandExecutionItem): HTMLElement {
    const container = el('div', 'item', 'item-command');

    // Header
    const header = el('div', 'item-command__header');
    const typeBadge = el('span', 'item__type-badge');
    typeBadge.textContent = 'Command';
    header.appendChild(typeBadge);

    const command = el('code', 'item-command__command', 'mono');
    command.textContent = item.command;
    header.appendChild(command);

    if (item.cwd) {
        const cwd = el('span', 'item-command__cwd', 'text-muted');
        cwd.textContent = item.cwd;
        header.appendChild(cwd);
    }

    const statusBadge = el('span', 'item__status');
    statusBadge.textContent = item.status;
    statusBadge.classList.add(statusToBadgeClass(item.status));
    header.appendChild(statusBadge);
    container.appendChild(header);

    // Output area
    const output = el('pre', 'item-command__output');
    output.dataset['role'] = 'output';
    if (item.aggregatedOutput) {
        output.textContent = item.aggregatedOutput;
    }
    container.appendChild(output);

    // Footer
    const footer = el('div', 'item-command__footer');

    if (item.exitCode !== null) {
        const exitCode = el('span', 'item-command__exit-code');
        exitCode.textContent = `Exit: ${item.exitCode}`;
        exitCode.classList.add(item.exitCode === 0 ? 'text-green' : 'text-red');
        footer.appendChild(exitCode);
    }

    if (item.durationMs !== null) {
        const duration = el('span', 'item-command__duration', 'text-muted');
        duration.textContent = formatDuration(item.durationMs);
        footer.appendChild(duration);
    }

    footer.dataset['role'] = 'footer';
    container.appendChild(footer);

    return container;
}

function renderFileChangeItem(item: FileChangeItem): HTMLElement {
    const container = el('div', 'item', 'item-file-change');

    const header = el('div', 'item__header');
    const typeBadge = el('span', 'item__type-badge');
    typeBadge.textContent = 'File Changes';
    header.appendChild(typeBadge);

    const statusBadge = el('span', 'item__status');
    statusBadge.textContent = item.status;
    statusBadge.classList.add(statusToBadgeClass(item.status));
    header.appendChild(statusBadge);
    container.appendChild(header);

    for (const change of item.changes) {
        container.appendChild(renderFileChange(change));
    }

    return container;
}

function renderFileChange(change: FileUpdateChange): HTMLElement {
    const fileEl = el('div', 'item-file-change__file');

    const typeSpan = el('span', 'item-file-change__type');
    typeSpan.textContent = change.kind.type;
    typeSpan.classList.add(fileChangeTypeClass(change.kind.type));
    fileEl.appendChild(typeSpan);

    const pathSpan = el('span', 'mono');
    pathSpan.textContent = change.path;
    fileEl.appendChild(pathSpan);

    if (change.diff) {
        const patchContainer = el('div', 'collapsible', 'collapsible--collapsed');

        const patchHeader = el('div', 'collapsible__header');
        patchHeader.textContent = '\u25b6 Show diff';
        patchHeader.setAttribute('role', 'button');
        patchHeader.setAttribute('aria-label', 'Toggle diff');
        patchHeader.addEventListener('click', () => {
            const isCollapsed = patchContainer.classList.contains('collapsible--collapsed');
            patchContainer.classList.toggle('collapsible--collapsed', !isCollapsed);
            patchContainer.classList.toggle('collapsible--expanded', isCollapsed);
            patchHeader.textContent = isCollapsed ? '\u25bc Hide diff' : '\u25b6 Show diff';
        });

        const patchContent = el('pre', 'collapsible__content', 'item-file-change__patch');
        patchContent.innerHTML = renderDiff(change.diff);

        patchContainer.appendChild(patchHeader);
        patchContainer.appendChild(patchContent);
        fileEl.appendChild(patchContainer);
    }

    return fileEl;
}

function renderDiff(patch: string): string {
    return patch
        .split('\n')
        .map((line) => {
            const escaped = escapeHtml(line);
            if (line.startsWith('@@')) {
                return `<span class="diff-hunk">${escaped}</span>`;
            }
            if (line.startsWith('+')) {
                return `<span class="diff-add">${escaped}</span>`;
            }
            if (line.startsWith('-')) {
                return `<span class="diff-del">${escaped}</span>`;
            }
            return escaped;
        })
        .join('\n');
}

/**
 * Extract text content from an MCP tool call result's content array.
 * Content items are JsonValue (could be null, string, object with type/text, etc).
 */
function extractMcpResultText(content: ReadonlyArray<unknown>): string {
    return content
        .filter((c): c is Record<string, unknown> => c != null && typeof c === 'object' && !Array.isArray(c))
        .filter((c) => c['type'] === 'text' && typeof c['text'] === 'string')
        .map((c) => c['text'] as string)
        .join('\n');
}

function renderMcpToolCallItem(item: McpToolCallItem): HTMLElement {
    const container = el('div', 'item', 'item-mcp');

    const header = el('div', 'item-mcp__header');
    const typeBadge = el('span', 'item__type-badge');
    typeBadge.textContent = 'MCP';
    header.appendChild(typeBadge);

    const name = el('span', 'mono');
    name.textContent = `${item.server}:${item.tool}`;
    header.appendChild(name);

    const statusBadge = el('span', 'item__status');
    statusBadge.textContent = item.status;
    statusBadge.classList.add(statusToBadgeClass(item.status));
    header.appendChild(statusBadge);
    container.appendChild(header);

    // Arguments (collapsible)
    if (item.arguments !== null && item.arguments !== undefined) {
        const argsContainer = el('div', 'collapsible', 'collapsible--collapsed');

        const argsHeader = el('div', 'collapsible__header');
        argsHeader.textContent = '\u25b6 Arguments';
        argsHeader.setAttribute('role', 'button');
        argsHeader.setAttribute('aria-label', 'Toggle arguments');
        argsHeader.addEventListener('click', () => {
            const isCollapsed = argsContainer.classList.contains('collapsible--collapsed');
            argsContainer.classList.toggle('collapsible--collapsed', !isCollapsed);
            argsContainer.classList.toggle('collapsible--expanded', isCollapsed);
            argsHeader.textContent = isCollapsed ? '\u25bc Arguments' : '\u25b6 Arguments';
        });

        const argsContent = el('pre', 'collapsible__content', 'item-mcp__args');
        try {
            argsContent.textContent = JSON.stringify(item.arguments, null, 2);
        } catch {
            argsContent.textContent = String(item.arguments);
        }

        argsContainer.appendChild(argsHeader);
        argsContainer.appendChild(argsContent);
        container.appendChild(argsContainer);
    }

    // Progress indicator
    const progress = el('div', 'item-mcp__progress');
    progress.dataset['role'] = 'progress';
    container.appendChild(progress);

    // Result area
    const result = el('div', 'item-mcp__result');
    result.dataset['role'] = 'result';
    if (item.result?.content) {
        const resultText = extractMcpResultText(item.result.content);
        if (resultText) {
            result.innerHTML = renderMarkdown(resultText);
        }
    }
    if (item.error) {
        result.classList.add('text-red');
        result.textContent = `Error: ${item.error.message}`;
    }
    container.appendChild(result);

    // Duration
    if (item.durationMs !== null) {
        const duration = el('span', 'item-mcp__duration', 'text-muted');
        duration.textContent = formatDuration(item.durationMs);
        container.appendChild(duration);
    }

    return container;
}

function renderDynamicToolCallItem(item: DynamicToolCallItem): HTMLElement {
    const container = el('div', 'item', 'item-mcp');

    const header = el('div', 'item-mcp__header');
    const typeBadge = el('span', 'item__type-badge');
    typeBadge.textContent = 'Tool';
    header.appendChild(typeBadge);

    const name = el('span', 'mono');
    name.textContent = item.tool;
    header.appendChild(name);

    const statusBadge = el('span', 'item__status');
    statusBadge.textContent = item.status;
    statusBadge.classList.add(statusToBadgeClass(item.status));
    header.appendChild(statusBadge);
    container.appendChild(header);

    // Arguments (collapsible)
    if (item.arguments !== null && item.arguments !== undefined) {
        const argsContainer = el('div', 'collapsible', 'collapsible--collapsed');

        const argsHeader = el('div', 'collapsible__header');
        argsHeader.textContent = '\u25b6 Arguments';
        argsHeader.setAttribute('role', 'button');
        argsHeader.setAttribute('aria-label', 'Toggle arguments');
        argsHeader.addEventListener('click', () => {
            const isCollapsed = argsContainer.classList.contains('collapsible--collapsed');
            argsContainer.classList.toggle('collapsible--collapsed', !isCollapsed);
            argsContainer.classList.toggle('collapsible--expanded', isCollapsed);
            argsHeader.textContent = isCollapsed ? '\u25bc Arguments' : '\u25b6 Arguments';
        });

        const argsContent = el('pre', 'collapsible__content', 'item-mcp__args');
        try {
            argsContent.textContent = JSON.stringify(item.arguments, null, 2);
        } catch {
            argsContent.textContent = String(item.arguments);
        }

        argsContainer.appendChild(argsHeader);
        argsContainer.appendChild(argsContent);
        container.appendChild(argsContainer);
    }

    // Result
    const result = el('div', 'item-mcp__result');
    result.dataset['role'] = 'result';
    if (item.contentItems) {
        const resultText = item.contentItems
            .filter((c): c is Extract<typeof c, { type: 'inputText' }> => c.type === 'inputText')
            .map((c) => c.text)
            .join('\n');
        if (resultText) {
            result.innerHTML = renderMarkdown(resultText);
        }
    }
    container.appendChild(result);

    // Duration
    if (item.durationMs !== null) {
        const duration = el('span', 'item-mcp__duration', 'text-muted');
        duration.textContent = formatDuration(item.durationMs);
        container.appendChild(duration);
    }

    return container;
}

function renderReasoningItem(item: ReasoningItem): HTMLElement {
    const container = el('div', 'item', 'item-reasoning', 'item-reasoning--collapsed');

    const toggle = el('div', 'item-reasoning__toggle');
    toggle.setAttribute('role', 'button');
    toggle.setAttribute('aria-label', 'Toggle reasoning');

    const chevron = el('span');
    chevron.textContent = '\u25b6';
    chevron.dataset['role'] = 'chevron';
    toggle.appendChild(chevron);

    const summarySpan = el('span');
    summarySpan.dataset['role'] = 'summary';
    summarySpan.textContent = item.summary.length > 0 ? item.summary.join(' ') : 'Thinking...';
    toggle.appendChild(summarySpan);

    toggle.addEventListener('click', () => {
        const isCollapsed = container.classList.contains('item-reasoning--collapsed');
        container.classList.toggle('item-reasoning--collapsed', !isCollapsed);
        container.classList.toggle('item-reasoning--expanded', isCollapsed);
        const chevronEl = container.querySelector('[data-role="chevron"]');
        if (chevronEl) {
            chevronEl.textContent = isCollapsed ? '\u25bc' : '\u25b6';
        }
    });

    container.appendChild(toggle);

    const content = el('div', 'item-reasoning__content');
    content.dataset['role'] = 'content';
    if (item.content.length > 0) {
        content.textContent = item.content.join('\n');
    }
    container.appendChild(content);

    return container;
}

function renderPlanItem(item: PlanItem): HTMLElement {
    const container = el('div', 'item', 'item-plan');
    const content = el('div', 'item-plan__content');
    content.dataset['role'] = 'content';
    content.innerHTML = renderMarkdown(item.text);
    container.appendChild(content);
    return container;
}

function renderWebSearchItem(item: WebSearchItem): HTMLElement {
    const container = el('div', 'item', 'item-web-search');
    const query = el('span', 'item-web-search__query');
    query.textContent = `\uD83D\uDD0D ${item.query}`;
    container.appendChild(query);
    return container;
}

function renderAgentMessageItem(item: AgentMessageItem): HTMLElement {
    // Agent messages are handled via streaming deltas, but if an item/started
    // fires with content already present, render it.
    const msg = el('div', 'message', 'message--assistant');
    const content = el('div', 'message__content');
    content.dataset['role'] = 'content';
    if (item.text) {
        content.innerHTML = renderMarkdown(item.text);
    }
    msg.appendChild(content);
    const meta = el('div', 'message__meta');
    msg.appendChild(meta);
    return msg;
}

/** Fallback renderer for item types we don't have a dedicated renderer for. */
function renderGenericItem(item: ThreadItem): HTMLElement {
    const container = el('div', 'item');

    const header = el('div', 'item__header');
    const typeBadge = el('span', 'item__type-badge');
    typeBadge.textContent = item.type;
    header.appendChild(typeBadge);

    const statusEl = el('span', 'item__status', 'text-muted');
    const itemStatus = (item as { status?: string }).status;
    if (itemStatus) {
        statusEl.textContent = itemStatus;
    }
    header.appendChild(statusEl);

    container.appendChild(header);
    return container;
}

// ==========================================================================
// Approval Renderers
// ==========================================================================

function renderCommandApproval(
    params: CommandApprovalRequest,
    respond: (result: unknown) => void,
    reject: (error: { code: number; message: string }) => void,
): HTMLElement {
    const container = el('div', 'approval', 'approval--pending');

    const desc = el('div', 'approval__description');
    desc.textContent = 'Command requires approval:';
    container.appendChild(desc);

    if (params.command) {
        const cmd = el('code', 'approval__command', 'mono');
        cmd.textContent = params.command;
        container.appendChild(cmd);
    }

    if (params.cwd) {
        const cwdEl = el('div', 'text-muted');
        cwdEl.textContent = `in ${params.cwd}`;
        container.appendChild(cwdEl);
    }

    if (params.reason) {
        const reasonEl = el('div', 'text-secondary');
        reasonEl.textContent = params.reason;
        container.appendChild(reasonEl);
    }

    const buttons = el('div', 'approval__buttons');
    const resolveApproval = (approved: boolean) => {
        container.classList.remove('approval--pending');
        container.classList.add('approval--resolved');
        for (const btn of buttons.querySelectorAll('button')) {
            (btn as HTMLButtonElement).disabled = true;
        }
        try {
            respond({ approved });
        } catch (err) {
            reject({ code: -1, message: err instanceof Error ? err.message : String(err) });
        }
    };

    const allowBtn = document.createElement('button');
    allowBtn.classList.add('approval__btn', 'approval__btn--allow');
    allowBtn.textContent = 'Allow';
    allowBtn.setAttribute('aria-label', 'Allow command');
    allowBtn.addEventListener('click', () => resolveApproval(true));

    const denyBtn = document.createElement('button');
    denyBtn.classList.add('approval__btn', 'approval__btn--deny');
    denyBtn.textContent = 'Deny';
    denyBtn.setAttribute('aria-label', 'Deny command');
    denyBtn.addEventListener('click', () => resolveApproval(false));

    buttons.appendChild(allowBtn);
    buttons.appendChild(denyBtn);
    container.appendChild(buttons);

    return container;
}

function renderFileChangeApproval(
    params: FileChangeApprovalRequest,
    respond: (result: unknown) => void,
    reject: (error: { code: number; message: string }) => void,
): HTMLElement {
    const container = el('div', 'approval', 'approval--pending');

    const desc = el('div', 'approval__description');
    desc.textContent = params.reason ?? 'File changes require approval';
    container.appendChild(desc);

    if (params.grantRoot) {
        const rootEl = el('div', 'text-muted');
        rootEl.textContent = `Write access requested for: ${params.grantRoot}`;
        container.appendChild(rootEl);
    }

    const buttons = el('div', 'approval__buttons');
    const resolveApproval = (approved: boolean) => {
        container.classList.remove('approval--pending');
        container.classList.add('approval--resolved');
        for (const btn of buttons.querySelectorAll('button')) {
            (btn as HTMLButtonElement).disabled = true;
        }
        try {
            respond({ approved });
        } catch (err) {
            reject({ code: -1, message: err instanceof Error ? err.message : String(err) });
        }
    };

    const allowBtn = document.createElement('button');
    allowBtn.classList.add('approval__btn', 'approval__btn--allow');
    allowBtn.textContent = 'Allow';
    allowBtn.setAttribute('aria-label', 'Allow file changes');
    allowBtn.addEventListener('click', () => resolveApproval(true));

    const denyBtn = document.createElement('button');
    denyBtn.classList.add('approval__btn', 'approval__btn--deny');
    denyBtn.textContent = 'Deny';
    denyBtn.setAttribute('aria-label', 'Deny file changes');
    denyBtn.addEventListener('click', () => resolveApproval(false));

    buttons.appendChild(allowBtn);
    buttons.appendChild(denyBtn);
    container.appendChild(buttons);

    return container;
}

function renderPermissionsApproval(
    params: PermissionsApprovalRequest,
    respond: (result: unknown) => void,
    reject: (error: { code: number; message: string }) => void,
): HTMLElement {
    const container = el('div', 'approval', 'approval--pending');

    const desc = el('div', 'approval__description');
    desc.textContent = 'Permissions required:';
    container.appendChild(desc);

    const permEl = el('pre', 'mono', 'text-muted');
    permEl.textContent = JSON.stringify(params.permissions, null, 2);
    container.appendChild(permEl);

    if (params.reason) {
        const reasonEl = el('div', 'text-secondary');
        reasonEl.textContent = params.reason;
        container.appendChild(reasonEl);
    }

    const buttons = el('div', 'approval__buttons');
    const resolveApproval = (approved: boolean) => {
        container.classList.remove('approval--pending');
        container.classList.add('approval--resolved');
        for (const btn of buttons.querySelectorAll('button')) {
            (btn as HTMLButtonElement).disabled = true;
        }
        try {
            respond({ approved });
        } catch (err) {
            reject({ code: -1, message: err instanceof Error ? err.message : String(err) });
        }
    };

    const allowBtn = document.createElement('button');
    allowBtn.classList.add('approval__btn', 'approval__btn--allow');
    allowBtn.textContent = 'Allow';
    allowBtn.setAttribute('aria-label', 'Allow permissions');
    allowBtn.addEventListener('click', () => resolveApproval(true));

    const denyBtn = document.createElement('button');
    denyBtn.classList.add('approval__btn', 'approval__btn--deny');
    denyBtn.textContent = 'Deny';
    denyBtn.setAttribute('aria-label', 'Deny permissions');
    denyBtn.addEventListener('click', () => resolveApproval(false));

    buttons.appendChild(allowBtn);
    buttons.appendChild(denyBtn);
    container.appendChild(buttons);

    return container;
}

function renderToolUserInput(
    params: ToolUserInputRequest,
    respond: (result: unknown) => void,
    reject: (error: { code: number; message: string }) => void,
): HTMLElement {
    const container = el('div', 'approval', 'approval--pending');

    const desc = el('div', 'approval__description');
    desc.textContent = 'Tool requires user input:';
    container.appendChild(desc);

    for (const question of params.questions) {
        const qEl = el('div', 'text-secondary');
        qEl.textContent = question.question || question.header;
        container.appendChild(qEl);
    }

    const inputEl = document.createElement('textarea');
    inputEl.classList.add('input-area__textarea');
    inputEl.rows = 2;
    inputEl.setAttribute('aria-label', 'Tool input');
    container.appendChild(inputEl);

    const buttons = el('div', 'approval__buttons');

    const submitBtn = document.createElement('button');
    submitBtn.classList.add('approval__btn', 'approval__btn--allow');
    submitBtn.textContent = 'Submit';
    submitBtn.setAttribute('aria-label', 'Submit input');
    submitBtn.addEventListener('click', () => {
        const value = inputEl.value;
        container.classList.remove('approval--pending');
        container.classList.add('approval--resolved');
        submitBtn.disabled = true;
        cancelBtn.disabled = true;
        inputEl.disabled = true;
        try {
            respond({ text: value });
        } catch (err) {
            reject({ code: -1, message: err instanceof Error ? err.message : String(err) });
        }
    });

    const cancelBtn = document.createElement('button');
    cancelBtn.classList.add('approval__btn', 'approval__btn--deny');
    cancelBtn.textContent = 'Cancel';
    cancelBtn.setAttribute('aria-label', 'Cancel input');
    cancelBtn.addEventListener('click', () => {
        container.classList.remove('approval--pending');
        container.classList.add('approval--resolved');
        submitBtn.disabled = true;
        cancelBtn.disabled = true;
        inputEl.disabled = true;
        reject({ code: -32000, message: 'User cancelled' });
    });

    buttons.appendChild(submitBtn);
    buttons.appendChild(cancelBtn);
    container.appendChild(buttons);

    return container;
}

function renderPatchApproval(
    params: PatchApprovalRequest,
    respond: (result: unknown) => void,
    reject: (error: { code: number; message: string }) => void,
): HTMLElement {
    const container = el('div', 'approval', 'approval--pending');

    const desc = el('div', 'approval__description');
    desc.textContent = params.reason ?? 'Patch requires approval:';
    container.appendChild(desc);

    for (const [filePath, change] of Object.entries(params.fileChanges)) {
        if (!change) continue;
        const fileEl = el('div', 'item-file-change__file');
        const typeSpan = el('span', 'item-file-change__type');
        typeSpan.textContent = change.type;
        typeSpan.classList.add(fileChangeTypeClass(change.type));
        fileEl.appendChild(typeSpan);

        const pathSpan = el('span', 'mono');
        pathSpan.textContent = filePath;
        fileEl.appendChild(pathSpan);

        const diffText = change.type === 'update' ? change.unified_diff : change.content;
        if (diffText) {
            const patchPre = el('pre', 'item-file-change__patch');
            patchPre.innerHTML = renderDiff(diffText);
            fileEl.appendChild(patchPre);
        }

        container.appendChild(fileEl);
    }

    if (params.grantRoot) {
        const rootEl = el('div', 'text-muted');
        rootEl.textContent = `Write access requested for: ${params.grantRoot}`;
        container.appendChild(rootEl);
    }

    const buttons = el('div', 'approval__buttons');
    const resolveApproval = (approved: boolean) => {
        container.classList.remove('approval--pending');
        container.classList.add('approval--resolved');
        for (const btn of buttons.querySelectorAll('button')) {
            (btn as HTMLButtonElement).disabled = true;
        }
        try {
            respond({ approved });
        } catch (err) {
            reject({ code: -1, message: err instanceof Error ? err.message : String(err) });
        }
    };

    const allowBtn = document.createElement('button');
    allowBtn.classList.add('approval__btn', 'approval__btn--allow');
    allowBtn.textContent = 'Allow';
    allowBtn.setAttribute('aria-label', 'Allow patch');
    allowBtn.addEventListener('click', () => resolveApproval(true));

    const denyBtn = document.createElement('button');
    denyBtn.classList.add('approval__btn', 'approval__btn--deny');
    denyBtn.textContent = 'Deny';
    denyBtn.setAttribute('aria-label', 'Deny patch');
    denyBtn.addEventListener('click', () => resolveApproval(false));

    buttons.appendChild(allowBtn);
    buttons.appendChild(denyBtn);
    container.appendChild(buttons);

    return container;
}

// ==========================================================================
// Policy Approval Handling (command & network policy from app-server-loader)
// ==========================================================================

function addPolicyApprovalMessage(
    description: string,
    respond: (decision: ApprovalDecision) => void,
): void {
    const container = el('div', 'approval', 'approval--pending');

    const desc = el('div', 'approval__description');
    desc.textContent = description;
    container.appendChild(desc);

    const buttons = el('div', 'approval__buttons');

    const disableAll = () => {
        container.classList.remove('approval--pending');
        container.classList.add('approval--resolved');
        for (const btn of buttons.querySelectorAll('button')) {
            (btn as HTMLButtonElement).disabled = true;
        }
    };

    const allowBtn = document.createElement('button');
    allowBtn.classList.add('approval__btn', 'approval__btn--allow');
    allowBtn.textContent = 'Allow';
    allowBtn.setAttribute('aria-label', 'Allow');
    allowBtn.addEventListener('click', () => { disableAll(); respond('allow'); });

    const allowSessionBtn = document.createElement('button');
    allowSessionBtn.classList.add('approval__btn', 'approval__btn--allow-session');
    allowSessionBtn.textContent = 'Allow (session)';
    allowSessionBtn.setAttribute('aria-label', 'Allow for session');
    allowSessionBtn.addEventListener('click', () => { disableAll(); respond('allow-session'); });

    const denyBtn = document.createElement('button');
    denyBtn.classList.add('approval__btn', 'approval__btn--deny');
    denyBtn.textContent = 'Deny';
    denyBtn.setAttribute('aria-label', 'Deny');
    denyBtn.addEventListener('click', () => { disableAll(); respond('deny'); });

    buttons.appendChild(allowBtn);
    buttons.appendChild(allowSessionBtn);
    buttons.appendChild(denyBtn);
    container.appendChild(buttons);

    messagesContainer.insertBefore(container, thinkingIndicator);
    autoScroll();
}

// ==========================================================================
// Helper: badge class from status
// ==========================================================================

function statusToBadgeClass(status: string): string {
    switch (status) {
        case 'completed':
            return 'badge--success';
        case 'running':
        case 'pending':
            return 'badge--info';
        case 'failed':
        case 'error':
        case 'cancelled':
            return 'badge--error';
        default:
            return 'badge--muted';
    }
}

function fileChangeTypeClass(changeType: string): string {
    switch (changeType) {
        case 'add':
            return 'text-green';
        case 'update':
            return 'text-blue';
        case 'delete':
            return 'text-red';
        default:
            return 'text-muted';
    }
}

// ==========================================================================
// Item Dispatch
// ==========================================================================

function renderItem(item: ThreadItem): HTMLElement {
    switch (item.type) {
        case 'commandExecution':
            return renderCommandItem(item);
        case 'fileChange':
            return renderFileChangeItem(item);
        case 'mcpToolCall':
            return renderMcpToolCallItem(item);
        case 'dynamicToolCall':
            return renderDynamicToolCallItem(item);
        case 'reasoning':
            return renderReasoningItem(item);
        case 'plan':
            return renderPlanItem(item);
        case 'webSearch':
            return renderWebSearchItem(item);
        case 'agentMessage':
            return renderAgentMessageItem(item);
        default:
            return renderGenericItem(item);
    }
}

// ==========================================================================
// Event Wiring
// ==========================================================================

function wireEvents(client: AppServerClient): void {
    // --- Turn lifecycle ---

    client.on('turn/started', (event: TurnStartedEvent) => {
        console.warn('[App Server] turn/started received', event);
        console.trace('[App Server] turn/started stack');
        state.turnId = event.turn.id;
        state.turnActive = true;
        currentAssistantContent = null;
        currentAssistantBuffer = '';
        assistantDeltaCount = 0;
        setStatus('thinking');
        showThinking();
        setInputEnabled(false);
    });

    client.on('turn/completed', (event: TurnCompletedEvent) => {
        state.turnActive = false;
        hideThinking();
        setStatus('connected');
        setInputEnabled(true);

        // Flush any pending assistant render
        if (assistantRenderTimer !== null) {
            clearTimeout(assistantRenderTimer);
            assistantRenderTimer = null;
        }
        flushAssistantRender();

        // Show turn duration in the last assistant message meta
        if (event.turn?.durationMs !== null && event.turn?.durationMs !== undefined) {
            const assistantMessages = messagesContainer.querySelectorAll('.message--assistant');
            const lastMsg = assistantMessages[assistantMessages.length - 1];
            if (lastMsg) {
                const meta = lastMsg.querySelector('.message__meta');
                if (meta) {
                    meta.textContent = formatDuration(event.turn.durationMs);
                }
            }
        }

        // Show error if turn failed
        if (event.turn?.error) {
            appendSystemMessage(`Turn error: ${event.turn.error.message}`);
        }

        currentAssistantContent = null;
        currentAssistantBuffer = '';
    });

    // --- Streaming content ---

    client.on('item/agentMessage/delta', (event: AgentMessageDelta) => {
        // If we have a tracked agentMessage item, update its content
        const tracked = state.items.get(event.itemId);
        if (tracked) {
            tracked.streamBuffer += event.delta;
            const contentEl = tracked.element.querySelector('[data-role="content"]');
            if (contentEl) {
                (contentEl as HTMLElement).innerHTML = renderMarkdown(tracked.streamBuffer);
                autoScroll();
                return;
            }
        }
        // Otherwise fall back to the global assistant message stream
        appendToAssistantMessage(event.delta);
    });

    client.on('item/commandExecution/outputDelta', (event: CommandExecutionOutputDeltaNotification) => {
        const tracked = state.items.get(event.itemId);
        if (!tracked) return;
        const outputEl = tracked.element.querySelector('[data-role="output"]');
        if (outputEl) {
            tracked.streamBuffer += event.delta;
            outputEl.textContent = tracked.streamBuffer;
            autoScroll();
        }
    });

    client.on('item/fileChange/outputDelta', (event: FileChangeOutputDeltaNotification) => {
        const tracked = state.items.get(event.itemId);
        if (!tracked) return;
        tracked.streamBuffer += event.delta;
        // File change deltas are appended as additional patch content
        autoScroll();
    });

    client.on('item/plan/delta', (event: PlanDeltaNotification) => {
        const tracked = state.items.get(event.itemId);
        if (!tracked) return;
        tracked.streamBuffer += event.delta;
        const contentEl = tracked.element.querySelector('[data-role="content"]');
        if (contentEl) {
            (contentEl as HTMLElement).innerHTML = renderMarkdown(tracked.streamBuffer);
            autoScroll();
        }
    });

    client.on('item/reasoning/textDelta', (event: ReasoningTextDeltaNotification) => {
        const tracked = state.items.get(event.itemId);
        if (!tracked) return;
        tracked.streamBuffer += event.delta;
        const contentEl = tracked.element.querySelector('[data-role="content"]');
        if (contentEl) {
            contentEl.textContent = tracked.streamBuffer;
            autoScroll();
        }
    });

    client.on('item/reasoning/summaryTextDelta', (event: ReasoningSummaryTextDeltaNotification) => {
        const tracked = state.items.get(event.itemId);
        if (!tracked) return;
        const summaryEl = tracked.element.querySelector('[data-role="summary"]');
        if (summaryEl) {
            // Append to existing summary text
            summaryEl.textContent = (summaryEl.textContent ?? '') + event.delta;
        }
    });

    client.on('item/mcpToolCall/progress', (event: McpToolCallProgressNotification) => {
        const tracked = state.items.get(event.itemId);
        if (!tracked) return;
        const progressEl = tracked.element.querySelector('[data-role="progress"]');
        if (progressEl) {
            progressEl.textContent = event.message;
            autoScroll();
        }
    });

    // --- Item lifecycle ---

    client.on('item/started', (event: ItemStartedEvent) => {
        const item = event.item;

        // For agentMessage items, integrate with the assistant message stream
        if (item.type === 'agentMessage') {
            const created = createAssistantMessage();
            currentAssistantContent = created.content;
            currentAssistantBuffer = item.text ?? '';
            if (currentAssistantBuffer) {
                currentAssistantContent.innerHTML = renderMarkdown(currentAssistantBuffer);
            }
            // Track it so streaming deltas can target it
            state.items.set(item.id, {
                element: created.element,
                streamBuffer: currentAssistantBuffer,
            });
            autoScroll();
            return;
        }

        const element = renderItem(item);
        state.items.set(item.id, { element, streamBuffer: '' });
        messagesContainer.insertBefore(element, thinkingIndicator);
        autoScroll();
    });

    client.on('item/completed', (event: ItemCompletedEvent) => {
        const tracked = state.items.get(event.item.id);
        if (tracked) {
            // Update the element with final state from the completed item
            updateCompletedItem(tracked, event.item);
            state.items.delete(event.item.id);
        }

        // If this was an agentMessage, clear current assistant tracking
        if (event.item.type === 'agentMessage') {
            currentAssistantContent = null;
            currentAssistantBuffer = '';
        }
    });

    // --- Guardian review ---

    client.on('item/autoApprovalReview/started', (event: GuardianReviewStartedNotification) => {
        if (!event.targetItemId) return;
        const tracked = state.items.get(event.targetItemId);
        if (tracked) {
            const indicator = el('div', 'hook-indicator', 'hook-indicator--running');
            indicator.textContent = 'Auto-approval review...';
            indicator.dataset['role'] = 'guardian';
            tracked.element.appendChild(indicator);
        }
    });

    client.on('item/autoApprovalReview/completed', (event: GuardianReviewCompletedNotification) => {
        if (!event.targetItemId) return;
        const tracked = state.items.get(event.targetItemId);
        if (tracked) {
            const indicator = tracked.element.querySelector('[data-role="guardian"]');
            if (indicator) {
                indicator.classList.remove('hook-indicator--running');
                indicator.classList.add('hook-indicator--completed');
                indicator.textContent = event.review.status === 'approved'
                    ? 'Auto-approved'
                    : 'Auto-approval denied';
            }
        }
    });

    // --- Hooks ---

    client.on('hook/started', (event: HookStartedNotification) => {
        const hookName = event.run.eventName;
        const indicator = el('div', 'hook-indicator', 'hook-indicator--running');
        indicator.textContent = `Hook: ${hookName}`;
        indicator.dataset['hookName'] = hookName;
        indicator.dataset['hookId'] = event.run.id;
        messagesContainer.insertBefore(indicator, thinkingIndicator);
        autoScroll();
    });

    client.on('hook/completed', (event: HookCompletedNotification) => {
        const hookId = event.run.id;
        const hookName = event.run.eventName;
        const indicator = messagesContainer.querySelector(`[data-hook-id="${hookId}"]`)
            ?? Array.from(messagesContainer.querySelectorAll(`[data-hook-name="${hookName}"]`)).pop();
        if (indicator) {
            indicator.classList.remove('hook-indicator--running');
            indicator.classList.add('hook-indicator--completed');
            const statusInfo = event.run.status === 'completed' ? '' : ` (${event.run.status})`;
            indicator.textContent = `Hook: ${hookName}${statusInfo}`;
        }
    });

    // --- Thread info ---

    client.on('thread/tokenUsage/updated', (event: ThreadTokenUsageUpdatedNotification) => {
        updateTokenUsage(event.tokenUsage);
    });

    client.on('thread/status/changed', (event: ThreadStatusChangedNotification) => {
        void event; // Status tracking for future use
    });

    // --- Account / Login ---

    client.on('account/login/completed', (event: AccountLoginCompletedNotification) => {
        if (event.success) {
            pendingLoginId = null;
            authenticated = true;
            // Fetch account details
            client.readAccount().then((resp: GetAccountResponse) => {
                currentAccount = resp.account;
                updateAccountDisplay();
                showChatUI();
            }).catch(() => {
                showChatUI(); // Still show chat even if account fetch fails
            });
        } else {
            // Show error on login screen
            const errorEl = loginScreenEl?.querySelector('.login-screen__error') as HTMLElement | null;
            if (errorEl) {
                errorEl.textContent = event.error ?? 'Login failed';
                errorEl.classList.add('login-screen__error--visible');
            }
            // Re-enable login buttons
            enableLoginButtons();
        }
    });

    client.on('account/updated', (event: AccountUpdatedNotification) => {
        // Refresh account display when auth state changes
        if (event.authMode) {
            authenticated = true;
            client.readAccount().then((resp: GetAccountResponse) => {
                currentAccount = resp.account;
                updateAccountDisplay();
            }).catch(() => { /* ignore */ });
        } else {
            authenticated = false;
            currentAccount = null;
            updateAccountDisplay();
        }
    });

    // --- Errors ---

    client.on('error', (event: ErrorNotification) => {
        console.error('[App Server Error]', event);
        appendSystemMessage(`Error: ${event.error?.message ?? 'unknown error'}`);
    });

    // --- Server requests (approval flows) ---

    client.onServerRequest(
        'item/commandExecution/requestApproval',
        (params: CommandApprovalRequest, respond, reject) => {
            const element = renderCommandApproval(params, respond, reject);
            messagesContainer.insertBefore(element, thinkingIndicator);
            autoScroll();
        },
    );

    client.onServerRequest(
        'item/fileChange/requestApproval',
        (params: FileChangeApprovalRequest, respond, reject) => {
            const element = renderFileChangeApproval(params, respond, reject);
            messagesContainer.insertBefore(element, thinkingIndicator);
            autoScroll();
        },
    );

    client.onServerRequest(
        'item/permissions/requestApproval',
        (params: PermissionsApprovalRequest, respond, reject) => {
            const element = renderPermissionsApproval(params, respond, reject);
            messagesContainer.insertBefore(element, thinkingIndicator);
            autoScroll();
        },
    );

    client.onServerRequest(
        'item/tool/requestUserInput',
        (params: ToolUserInputRequest, respond, reject) => {
            const element = renderToolUserInput(params, respond, reject);
            messagesContainer.insertBefore(element, thinkingIndicator);
            autoScroll();
            // Focus the input
            const textareaEl = element.querySelector('textarea');
            if (textareaEl) {
                (textareaEl as HTMLTextAreaElement).focus();
            }
        },
    );

    client.onServerRequest(
        'applyPatchApproval',
        (params: PatchApprovalRequest, respond, reject) => {
            const element = renderPatchApproval(params, respond, reject);
            messagesContainer.insertBefore(element, thinkingIndicator);
            autoScroll();
        },
    );
}

// ==========================================================================
// Item Completion Updates
// ==========================================================================

function updateCompletedItem(tracked: TrackedItem, item: ThreadItem): void {
    // Update status badges
    const statusEl = tracked.element.querySelector('.item__status');
    const itemStatus = (item as { status?: string }).status;
    if (statusEl && itemStatus) {
        statusEl.textContent = itemStatus;
        // Reset badge classes
        statusEl.classList.remove('badge--info', 'badge--success', 'badge--error', 'badge--muted', 'badge--warning');
        statusEl.classList.add(statusToBadgeClass(itemStatus));
    }

    switch (item.type) {
        case 'commandExecution': {
            const cmd = item;
            // Update output with final aggregated output
            if (cmd.aggregatedOutput) {
                const outputEl = tracked.element.querySelector('[data-role="output"]');
                if (outputEl) {
                    outputEl.textContent = cmd.aggregatedOutput;
                }
            }
            // Update footer
            const footer = tracked.element.querySelector('[data-role="footer"]');
            if (footer) {
                footer.innerHTML = '';
                if (cmd.exitCode !== null) {
                    const exitCode = el('span', 'item-command__exit-code');
                    exitCode.textContent = `Exit: ${cmd.exitCode}`;
                    exitCode.classList.add(cmd.exitCode === 0 ? 'text-green' : 'text-red');
                    footer.appendChild(exitCode);
                }
                if (cmd.durationMs !== null) {
                    const duration = el('span', 'item-command__duration', 'text-muted');
                    duration.textContent = formatDuration(cmd.durationMs);
                    footer.appendChild(duration);
                }
            }
            break;
        }
        case 'mcpToolCall': {
            const mcp = item;
            // Update result
            const resultEl = tracked.element.querySelector('[data-role="result"]');
            if (resultEl) {
                (resultEl as HTMLElement).innerHTML = '';
                if (mcp.result?.content) {
                    const resultText = extractMcpResultText(mcp.result.content);
                    if (resultText) {
                        (resultEl as HTMLElement).innerHTML = renderMarkdown(resultText);
                    }
                }
                if (mcp.error) {
                    resultEl.classList.add('text-red');
                    resultEl.textContent = `Error: ${mcp.error.message}`;
                }
            }
            // Clear progress
            const progressEl = tracked.element.querySelector('[data-role="progress"]');
            if (progressEl) {
                progressEl.textContent = '';
            }
            // Add duration
            if (mcp.durationMs !== null) {
                const existing = tracked.element.querySelector('.item-mcp__duration');
                if (existing) {
                    existing.textContent = formatDuration(mcp.durationMs);
                } else {
                    const duration = el('span', 'item-mcp__duration', 'text-muted');
                    duration.textContent = formatDuration(mcp.durationMs);
                    tracked.element.appendChild(duration);
                }
            }
            break;
        }
        case 'dynamicToolCall': {
            const dtc = item;
            const resultEl = tracked.element.querySelector('[data-role="result"]');
            if (resultEl && dtc.contentItems) {
                const resultText = dtc.contentItems
                    .filter((c): c is Extract<typeof c, { type: 'inputText' }> => c.type === 'inputText')
                    .map((c) => c.text)
                    .join('\n');
                if (resultText) {
                    (resultEl as HTMLElement).innerHTML = renderMarkdown(resultText);
                }
            }
            if (dtc.durationMs !== null) {
                const existing = tracked.element.querySelector('.item-mcp__duration');
                if (existing) {
                    existing.textContent = formatDuration(dtc.durationMs);
                } else {
                    const duration = el('span', 'item-mcp__duration', 'text-muted');
                    duration.textContent = formatDuration(dtc.durationMs);
                    tracked.element.appendChild(duration);
                }
            }
            break;
        }
        case 'agentMessage': {
            const agentMsg = item;
            const contentEl = tracked.element.querySelector('[data-role="content"]') ?? tracked.element.querySelector('.message__content');
            if (contentEl && agentMsg.text) {
                (contentEl as HTMLElement).innerHTML = renderMarkdown(agentMsg.text);
            }
            break;
        }
        case 'reasoning': {
            const reasoning = item;
            const summaryEl = tracked.element.querySelector('[data-role="summary"]');
            if (summaryEl && reasoning.summary.length > 0) {
                summaryEl.textContent = reasoning.summary.join(' ');
            }
            const contentEl = tracked.element.querySelector('[data-role="content"]');
            if (contentEl && reasoning.content.length > 0) {
                contentEl.textContent = reasoning.content.join('\n');
            }
            break;
        }
        case 'fileChange': {
            // Re-render with complete data
            const newElement = renderFileChangeItem(item);
            tracked.element.replaceWith(newElement);
            tracked.element = newElement;
            break;
        }
        default:
            break;
    }

    autoScroll();
}

// ==========================================================================
// Boot Sequence
// ==========================================================================

async function boot(): Promise<void> {
    buildUI();

    setStatus('booting', 'Initializing sandbox...');

    try {
        setStatus('booting', 'Loading WASM runtime...');
        const handle = await launchAppServer();

        setStatus('booting', 'Starting app server...');

        // Register policy approval callbacks
        handle.onCommandApproval((program, args, cwd, respond) => {
            const cmd = `${program} ${args.join(' ')}`;
            addPolicyApprovalMessage(`Command requires approval:\n${cmd}\nin ${cwd}`, respond);
        });

        handle.onNetworkApproval((url, method, respond) => {
            addPolicyApprovalMessage(`Network request requires approval:\n${method} ${url}`, respond);
        });

        setStatus('booting', 'Creating client...');
        state.client = new AppServerClient(handle);
        wireEvents(state.client);

        // Check auth status before showing chat
        try {
            const authStatus = await state.client.getAuthStatus();
            if (authStatus.authMethod) {
                authenticated = true;
                // Load account info
                const accountResponse = await state.client.readAccount();
                currentAccount = accountResponse.account;
                updateAccountDisplay();
                showChatUI();
            } else {
                showLoginScreen();
            }
        } catch {
            // Auth check failed, show login screen
            showLoginScreen();
        }
    } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        setStatus('error', `Boot failed: ${msg}`);
        appendSystemMessage(`Failed to start app server: ${msg}`);
        console.error('[App Server Boot]', err);
    }
}

boot().catch(console.error);
