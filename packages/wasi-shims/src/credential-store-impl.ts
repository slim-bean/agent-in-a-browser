/**
 * Browser Credential Store Shim.
 *
 * Implements the host-side of the WIT credential-store interface.
 * The WASM component calls `load`, `save`, and `delete-credential` and this
 * shim routes them through two tiers:
 *
 *   Tier 1 — PasswordCredential API (Chrome, for browser password-manager UX)
 *   Tier 2 — OPFS + WebCrypto (universal fallback, encrypted at rest)
 *
 * OPFS is always the durable source of truth. PasswordCredential is a
 * best-effort write-through for browsers that support it.
 *
 * In JSPI mode, the async APIs suspend the WASM stack while Promises resolve.
 */

// ============================================================================
// Type declarations for PasswordCredential (not in lib.dom.d.ts)
// ============================================================================

interface PasswordCredentialData {
    id: string;
    password: string;
    name?: string;
}

interface PasswordCredentialInstance extends Credential {
    readonly id: string;
    readonly password: string;
    readonly name: string;
}

interface PasswordCredentialConstructor {
    new (data: PasswordCredentialData): PasswordCredentialInstance;
    prototype: PasswordCredentialInstance;
}

declare const PasswordCredential: PasswordCredentialConstructor | undefined;

interface CredentialRequestOptionsWithPassword extends CredentialRequestOptions {
    password?: boolean;
    mediation?: CredentialMediationRequirement;
}

// ============================================================================
// Constants
// ============================================================================

const DB_NAME = 'edge-agent-credential-keys';
const KEY_STORE = 'encryption-keys';
const MASTER_KEY_ID = 'master-key';

// ============================================================================
// In-memory credential cache
// ============================================================================

let credentialCache: Record<string, string> | null = null;
let useCredentialAPI = false;

// ============================================================================
// IndexedDB helpers for CryptoKey storage
// ============================================================================

function openDB(): Promise<IDBDatabase> {
    return new Promise((resolve, reject) => {
        const request = indexedDB.open(DB_NAME, 1);
        request.onupgradeneeded = () => {
            request.result.createObjectStore(KEY_STORE);
        };
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
    });
}

function getKeyFromDB(db: IDBDatabase): Promise<CryptoKey | null> {
    return new Promise((resolve, reject) => {
        const tx = db.transaction(KEY_STORE, 'readonly');
        const store = tx.objectStore(KEY_STORE);
        const request = store.get(MASTER_KEY_ID);
        request.onsuccess = () => {
            const result: unknown = request.result;
            if (result instanceof CryptoKey) {
                resolve(result);
            } else {
                resolve(null);
            }
        };
        request.onerror = () => reject(request.error);
    });
}

function storeKeyInDB(db: IDBDatabase, key: CryptoKey): Promise<void> {
    return new Promise((resolve, reject) => {
        const tx = db.transaction(KEY_STORE, 'readwrite');
        const store = tx.objectStore(KEY_STORE);
        const request = store.put(key, MASTER_KEY_ID);
        request.onsuccess = () => resolve();
        request.onerror = () => reject(request.error);
    });
}

// ============================================================================
// WebCrypto key management
// ============================================================================

async function getOrCreateKey(): Promise<CryptoKey> {
    const db = await openDB();
    try {
        const existing = await getKeyFromDB(db);
        if (existing) return existing;

        // Generate new AES-GCM 256-bit key (non-exportable!)
        const key = await crypto.subtle.generateKey(
            { name: 'AES-GCM', length: 256 },
            false, // extractable: false -- key material can never be read by JS
            ['encrypt', 'decrypt'],
        );
        await storeKeyInDB(db, key);
        return key;
    } finally {
        db.close();
    }
}

// ============================================================================
// WebCrypto encrypt / decrypt
// ============================================================================

async function encrypt(key: CryptoKey, plaintext: string): Promise<ArrayBuffer> {
    const iv = crypto.getRandomValues(new Uint8Array(12)); // 96-bit IV for AES-GCM
    const encoded = new TextEncoder().encode(plaintext);
    const ciphertext = await crypto.subtle.encrypt(
        { name: 'AES-GCM', iv },
        key,
        encoded,
    );
    // Prepend IV to ciphertext
    const result = new Uint8Array(iv.length + ciphertext.byteLength);
    result.set(iv, 0);
    result.set(new Uint8Array(ciphertext), iv.length);
    return result.buffer;
}

async function decrypt(key: CryptoKey, data: ArrayBuffer): Promise<string> {
    const bytes = new Uint8Array(data);
    const iv = bytes.slice(0, 12);
    const ciphertext = bytes.slice(12);
    const plaintext = await crypto.subtle.decrypt(
        { name: 'AES-GCM', iv },
        key,
        ciphertext,
    );
    return new TextDecoder().decode(plaintext);
}

// ============================================================================
// OPFS credential storage (Tier 2 -- always used as durable store)
// ============================================================================

async function loadAllCredentials(): Promise<Record<string, string>> {
    const key = await getOrCreateKey();
    const root = await navigator.storage.getDirectory();
    try {
        const codexDir = await root.getDirectoryHandle('.codex', { create: false });
        const fileHandle = await codexDir.getFileHandle('credentials.enc', { create: false });
        const file = await fileHandle.getFile();
        const blob = await file.arrayBuffer();
        if (blob.byteLength === 0) return {};
        const json = await decrypt(key, blob);
        const parsed: unknown = JSON.parse(json);
        if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
            return {};
        }
        // Validate that all values are strings
        const result: Record<string, string> = {};
        for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
            if (typeof v === 'string') {
                result[k] = v;
            }
        }
        return result;
    } catch (_e: unknown) {
        return {}; // No credentials yet or decryption failed
    }
}

async function saveAllCredentials(creds: Record<string, string>): Promise<void> {
    const key = await getOrCreateKey();
    const root = await navigator.storage.getDirectory();
    const codexDir = await root.getDirectoryHandle('.codex', { create: true });
    const fileHandle = await codexDir.getFileHandle('credentials.enc', { create: true });
    const encrypted = await encrypt(key, JSON.stringify(creds));
    const writable = await fileHandle.createWritable();
    await writable.write(encrypted);
    await writable.close();
}

// ============================================================================
// Tier 1: PasswordCredential API (Chrome)
// ============================================================================

async function saveViaCredentialAPI(
    service: string,
    account: string,
    value: string,
): Promise<boolean> {
    if (typeof PasswordCredential === 'undefined') return false;
    try {
        const cred = new PasswordCredential({
            id: `${service}:${account}`,
            password: value,
            name: `${service} - ${account}`,
        });
        await navigator.credentials.store(cred);
        return true;
    } catch (_e: unknown) {
        return false;
    }
}

async function loadViaCredentialAPI(
    service: string,
    account: string,
): Promise<string | null> {
    if (typeof PasswordCredential === 'undefined') return null;
    try {
        const options: CredentialRequestOptionsWithPassword = {
            password: true,
            mediation: 'silent',
        };
        const cred = await navigator.credentials.get(options);
        if (
            cred !== null &&
            'password' in cred &&
            'id' in cred &&
            cred.id === `${service}:${account}` &&
            typeof (cred as PasswordCredentialInstance).password === 'string'
        ) {
            return (cred as PasswordCredentialInstance).password;
        }
        return null;
    } catch (_e: unknown) {
        return null;
    }
}

// ============================================================================
// Cache initialization
// ============================================================================

async function ensureLoaded(): Promise<Record<string, string>> {
    if (credentialCache !== null) return credentialCache;

    // Detect PasswordCredential support
    useCredentialAPI = typeof PasswordCredential !== 'undefined';

    // Always load from OPFS as source of truth
    credentialCache = await loadAllCredentials();
    return credentialCache;
}

// ============================================================================
// WIT-exported functions
// ============================================================================

/**
 * Load a credential for the given service and account.
 *
 * Returns the stored value, or undefined if no credential exists.
 * Tries PasswordCredential API first (Chrome), falls back to OPFS.
 */
export async function load(
    service: string,
    account: string,
): Promise<string | undefined> {
    const creds = await ensureLoaded();
    const compositeKey = `${service}:${account}`;

    // Check in-memory cache (sourced from OPFS)
    const cached = creds[compositeKey];
    if (cached !== undefined) return cached;

    // Attempt PasswordCredential API as a secondary lookup
    if (useCredentialAPI) {
        const fromAPI = await loadViaCredentialAPI(service, account);
        if (fromAPI !== null) {
            // Backfill into OPFS cache for consistency
            creds[compositeKey] = fromAPI;
            await saveAllCredentials(creds);
            return fromAPI;
        }
    }

    return undefined;
}

/**
 * Save a credential for the given service and account.
 *
 * Persists to OPFS (encrypted with AES-GCM) and optionally to
 * the PasswordCredential API for browser password-manager integration.
 */
export async function save(
    service: string,
    account: string,
    value: string,
): Promise<void> {
    const creds = await ensureLoaded();
    const compositeKey = `${service}:${account}`;
    creds[compositeKey] = value;

    // Save to OPFS (always, this is the durable store)
    await saveAllCredentials(creds);

    // Also save to PasswordCredential API if available (for browser UX)
    if (useCredentialAPI) {
        await saveViaCredentialAPI(service, account, value);
    }
}

/**
 * Delete a credential for the given service and account.
 *
 * Returns true if a credential was found and removed, false otherwise.
 * Note: PasswordCredential API does not support deletion, so we only
 * remove from OPFS. The browser password manager entry will remain
 * until the user manually removes it.
 */
export async function deleteCredential(
    service: string,
    account: string,
): Promise<boolean> {
    const creds = await ensureLoaded();
    const compositeKey = `${service}:${account}`;
    if (!(compositeKey in creds)) return false;
    delete creds[compositeKey];
    await saveAllCredentials(creds);
    return true;
}
