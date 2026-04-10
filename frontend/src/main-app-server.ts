/**
 * Main entry point for the App Server chat UI.
 *
 * A minimal vanilla-TypeScript frontend that proves the full WASM app-server
 * pipeline: user types a message -> WASM processes it -> agent responds with
 * streamed events -> UI renders them.
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
    type ThreadStartResponse,
} from './wasm/app-server/protocol-client.js';

// ==========================================================================
// State
// ==========================================================================

let client: AppServerClient | null = null;
let threadId: string | null = null;
let currentAssistantBubble: HTMLElement | null = null;
let inputEnabled = false;

// ==========================================================================
// DOM References (populated in buildUI)
// ==========================================================================

let statusBar: HTMLElement;
let messagesContainer: HTMLElement;
let inputForm: HTMLFormElement;
let inputField: HTMLInputElement;
let approvalPanel: HTMLElement;
let approvalLabel: HTMLElement;
let approveBtn: HTMLButtonElement;
let denyBtn: HTMLButtonElement;

// Current pending approval request id
let pendingApprovalId: string | null = null;

// ==========================================================================
// UI Construction
// ==========================================================================

function buildUI(): void {
    const app = document.getElementById('app')!;

    // --- Status Bar ---
    statusBar = document.createElement('div');
    Object.assign(statusBar.style, {
        padding: '8px 16px',
        background: '#16161e',
        borderBottom: '1px solid #292e42',
        fontSize: '12px',
        color: '#565f89',
        flexShrink: '0',
    });
    statusBar.textContent = 'Booting WASM...';
    app.appendChild(statusBar);

    // --- Messages Area ---
    messagesContainer = document.createElement('div');
    Object.assign(messagesContainer.style, {
        flex: '1',
        overflowY: 'auto',
        padding: '16px',
        display: 'flex',
        flexDirection: 'column',
        gap: '12px',
    });
    app.appendChild(messagesContainer);

    // --- Approval Panel (hidden by default) ---
    approvalPanel = document.createElement('div');
    Object.assign(approvalPanel.style, {
        display: 'none',
        padding: '12px 16px',
        background: '#1e2030',
        borderTop: '1px solid #292e42',
        flexShrink: '0',
    });

    approvalLabel = document.createElement('div');
    Object.assign(approvalLabel.style, {
        marginBottom: '8px',
        color: '#e0af68',
        fontSize: '13px',
        whiteSpace: 'pre-wrap',
    });
    approvalPanel.appendChild(approvalLabel);

    const btnRow = document.createElement('div');
    Object.assign(btnRow.style, { display: 'flex', gap: '8px' });

    approveBtn = document.createElement('button');
    approveBtn.textContent = 'Approve';
    styleButton(approveBtn, '#9ece6a', '#1a1b26');
    approveBtn.addEventListener('click', () => void handleApprove());

    denyBtn = document.createElement('button');
    denyBtn.textContent = 'Deny';
    styleButton(denyBtn, '#f7768e', '#1a1b26');
    denyBtn.addEventListener('click', () => void handleDeny());

    btnRow.appendChild(approveBtn);
    btnRow.appendChild(denyBtn);
    approvalPanel.appendChild(btnRow);
    app.appendChild(approvalPanel);

    // --- Input Area ---
    inputForm = document.createElement('form');
    Object.assign(inputForm.style, {
        display: 'flex',
        padding: '12px 16px',
        gap: '8px',
        background: '#16161e',
        borderTop: '1px solid #292e42',
        flexShrink: '0',
    });

    inputField = document.createElement('input');
    inputField.type = 'text';
    inputField.placeholder = 'Loading...';
    inputField.disabled = true;
    inputField.autocomplete = 'off';
    Object.assign(inputField.style, {
        flex: '1',
        padding: '8px 12px',
        background: '#1a1b26',
        border: '1px solid #292e42',
        borderRadius: '6px',
        color: '#c0caf5',
        fontFamily: 'inherit',
        fontSize: '14px',
        outline: 'none',
    });

    const sendBtn = document.createElement('button');
    sendBtn.type = 'submit';
    sendBtn.textContent = 'Send';
    styleButton(sendBtn, '#7aa2f7', '#1a1b26');

    inputForm.appendChild(inputField);
    inputForm.appendChild(sendBtn);
    inputForm.addEventListener('submit', (e) => void handleSubmit(e));
    app.appendChild(inputForm);
}

function styleButton(btn: HTMLButtonElement, bg: string, fg: string): void {
    Object.assign(btn.style, {
        padding: '8px 16px',
        background: bg,
        color: fg,
        border: 'none',
        borderRadius: '6px',
        fontFamily: 'inherit',
        fontSize: '13px',
        fontWeight: '600',
        cursor: 'pointer',
    });
}

// ==========================================================================
// Message Rendering
// ==========================================================================

function appendBubble(role: 'user' | 'assistant' | 'system', text: string): HTMLElement {
    const bubble = document.createElement('div');
    Object.assign(bubble.style, {
        padding: '10px 14px',
        borderRadius: '8px',
        maxWidth: '80%',
        whiteSpace: 'pre-wrap',
        wordBreak: 'break-word',
        lineHeight: '1.5',
    });

    if (role === 'user') {
        Object.assign(bubble.style, {
            alignSelf: 'flex-end',
            background: '#283457',
            color: '#c0caf5',
        });
    } else if (role === 'assistant') {
        Object.assign(bubble.style, {
            alignSelf: 'flex-start',
            background: '#1e2030',
            color: '#c0caf5',
        });
    } else {
        Object.assign(bubble.style, {
            alignSelf: 'center',
            background: 'transparent',
            color: '#565f89',
            fontSize: '12px',
        });
    }

    bubble.textContent = text;
    messagesContainer.appendChild(bubble);
    messagesContainer.scrollTop = messagesContainer.scrollHeight;
    return bubble;
}

function appendItemIndicator(item: ItemStartedEvent): HTMLElement {
    const el = document.createElement('div');
    Object.assign(el.style, {
        alignSelf: 'flex-start',
        padding: '6px 12px',
        borderRadius: '6px',
        background: '#1e2030',
        border: '1px solid #292e42',
        color: '#7aa2f7',
        fontSize: '12px',
    });
    el.dataset['itemId'] = item.item.id;
    el.textContent = `[${item.item.type}] ${item.item.id}`;
    messagesContainer.appendChild(el);
    messagesContainer.scrollTop = messagesContainer.scrollHeight;
    return el;
}

// ==========================================================================
// Input / Form Handling
// ==========================================================================

function setInputEnabled(enabled: boolean): void {
    inputEnabled = enabled;
    inputField.disabled = !enabled;
    inputField.placeholder = enabled ? 'Type a message...' : 'Agent is thinking...';
    if (enabled) {
        inputField.focus();
    }
}

async function handleSubmit(e: Event): Promise<void> {
    e.preventDefault();
    if (!client || !inputEnabled) return;

    const text = inputField.value.trim();
    if (!text) return;

    inputField.value = '';
    appendBubble('user', text);
    setInputEnabled(false);

    try {
        if (!threadId) {
            const threadResponse: ThreadStartResponse = await client.startThread();
            threadId = threadResponse.thread.id;
            appendBubble('system', `Thread ${threadId.slice(0, 8)}... created`);
        }

        currentAssistantBubble = null;
        await client.startTurn(threadId, text);
    } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        appendBubble('system', `Error: ${msg}`);
        setInputEnabled(true);
    }
}

// ==========================================================================
// Approval Handling
// ==========================================================================

function showApproval(req: CommandApprovalRequest, requestId: string): void {
    pendingApprovalId = requestId;
    const desc = req.command
        ? `Command: ${req.command}${req.cwd ? ` (in ${req.cwd})` : ''}`
        : (req.reason ?? 'Command approval required');
    approvalLabel.textContent = desc;
    approvalPanel.style.display = 'block';
}

function hideApproval(): void {
    pendingApprovalId = null;
    approvalPanel.style.display = 'none';
}

async function handleApprove(): Promise<void> {
    if (!client || !pendingApprovalId) return;
    const id = pendingApprovalId;
    hideApproval();
    try {
        await client.approveCommand(id);
    } catch (err) {
        appendBubble('system', `Approve failed: ${err instanceof Error ? err.message : String(err)}`);
    }
}

async function handleDeny(): Promise<void> {
    if (!client || !pendingApprovalId) return;
    const id = pendingApprovalId;
    hideApproval();
    try {
        await client.denyCommand(id);
    } catch (err) {
        appendBubble('system', `Deny failed: ${err instanceof Error ? err.message : String(err)}`);
    }
}

// ==========================================================================
// Policy Approval Handling (command & network policy)
// ==========================================================================

function addApprovalMessage(
    description: string,
    respond: (decision: ApprovalDecision) => void,
): void {
    const bubble = document.createElement('div');
    Object.assign(bubble.style, {
        alignSelf: 'flex-start',
        padding: '10px 14px',
        borderRadius: '8px',
        maxWidth: '80%',
        background: '#1e2030',
        border: '1px solid #e0af68',
        color: '#c0caf5',
    });

    const label = document.createElement('div');
    Object.assign(label.style, {
        marginBottom: '8px',
        color: '#e0af68',
        fontSize: '13px',
        whiteSpace: 'pre-wrap',
    });
    label.textContent = description;
    bubble.appendChild(label);

    const btnRow = document.createElement('div');
    Object.assign(btnRow.style, { display: 'flex', gap: '8px' });

    const allowBtn = document.createElement('button');
    allowBtn.textContent = 'Allow';
    styleButton(allowBtn, '#9ece6a', '#1a1b26');

    const allowSessionBtn = document.createElement('button');
    allowSessionBtn.textContent = 'Allow (session)';
    styleButton(allowSessionBtn, '#7aa2f7', '#1a1b26');

    const denyBtnEl = document.createElement('button');
    denyBtnEl.textContent = 'Deny';
    styleButton(denyBtnEl, '#f7768e', '#1a1b26');

    const disableButtons = () => {
        allowBtn.disabled = true;
        allowSessionBtn.disabled = true;
        denyBtnEl.disabled = true;
        Object.assign(bubble.style, { opacity: '0.6' });
    };

    allowBtn.addEventListener('click', () => {
        disableButtons();
        respond('allow');
    });

    allowSessionBtn.addEventListener('click', () => {
        disableButtons();
        respond('allow-session');
    });

    denyBtnEl.addEventListener('click', () => {
        disableButtons();
        respond('deny');
    });

    btnRow.appendChild(allowBtn);
    btnRow.appendChild(allowSessionBtn);
    btnRow.appendChild(denyBtnEl);
    bubble.appendChild(btnRow);

    messagesContainer.appendChild(bubble);
    messagesContainer.scrollTop = messagesContainer.scrollHeight;
}

// ==========================================================================
// Event Wiring
// ==========================================================================

function wireEvents(c: AppServerClient): void {
    c.onTurnStarted((_event: TurnStartedEvent) => {
        currentAssistantBubble = null;
    });

    c.onAgentMessage((event: AgentMessageDelta) => {
        if (!currentAssistantBubble) {
            currentAssistantBubble = appendBubble('assistant', '');
        }
        currentAssistantBubble.textContent += event.delta;
        messagesContainer.scrollTop = messagesContainer.scrollHeight;
    });

    c.onItemStarted((event: ItemStartedEvent) => {
        appendItemIndicator(event);
    });

    c.onItemCompleted((event: ItemCompletedEvent) => {
        const el = messagesContainer.querySelector(`[data-item-id="${event.item.id}"]`);
        if (el) {
            const status = event.item['status'] as string | undefined;
            (el as HTMLElement).style.borderColor = status === 'error' ? '#f7768e' : '#9ece6a';
        }
    });

    c.onTurnCompleted((_event: TurnCompletedEvent) => {
        currentAssistantBubble = null;
        setInputEnabled(true);
    });

    c.onApprovalRequired((req: CommandApprovalRequest, requestId: string) => {
        showApproval(req, requestId);
    });
}

// ==========================================================================
// Boot Sequence
// ==========================================================================

async function boot(): Promise<void> {
    buildUI();

    statusBar.textContent = 'Initializing WASM runtime...';

    try {
        const handle = await launchAppServer();
        statusBar.textContent = 'WASM ready. Creating client...';

        // Register policy approval callbacks before creating the client
        handle.onCommandApproval((program, args, cwd, respond) => {
            const cmd = `${program} ${args.join(' ')}`;
            addApprovalMessage(`Command requires approval:\n${cmd}\nin ${cwd}`, respond);
        });

        handle.onNetworkApproval((url, method, respond) => {
            addApprovalMessage(`Network request requires approval:\n${method} ${url}`, respond);
        });

        client = new AppServerClient(handle);
        wireEvents(client);

        statusBar.textContent = 'Connected';
        statusBar.style.color = '#9ece6a';
        setInputEnabled(true);
    } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        statusBar.textContent = `Boot failed: ${msg}`;
        statusBar.style.color = '#f7768e';
        appendBubble('system', `Failed to start app server: ${msg}`);
        console.error('[App Server Boot]', err);
    }
}

boot().catch(console.error);
