#!/usr/bin/env node

/**
 * Generates TypeScript protocol maps from the Rust app-server-protocol definitions.
 *
 * Parses the `client_request_definitions!`, `server_request_definitions!`, and
 * `server_notification_definitions!` macros in common.rs and emits a typed map
 * that constrains JSON-RPC method names and their associated params/response
 * types at compile time.
 *
 * Usage:
 *   node scripts/generate-protocol-map.mjs
 *
 * Output:
 *   frontend/src/wasm/app-server/protocol-map.generated.ts
 */

import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, '..');

const COMMON_RS = resolve(
    ROOT,
    'runtime/codex-upstream/codex-rs/app-server-protocol/src/protocol/common.rs',
);
const SCHEMA_ROOT = resolve(
    ROOT,
    'runtime/codex-upstream/codex-rs/app-server-protocol/schema/typescript',
);
const SCHEMA_V2 = resolve(SCHEMA_ROOT, 'v2');
const OUTPUT = resolve(
    ROOT,
    'frontend/src/wasm/app-server/protocol-map.generated.ts',
);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** PascalCase → camelCase (matches serde rename_all = "camelCase") */
function toCamelCase(s) {
    return s[0].toLowerCase() + s.slice(1);
}

/**
 * Check whether a TypeScript type file exists in the generated schema.
 * Returns 'v2' | 'root' | null.
 */
function findTypeLocation(typeName) {
    if (existsSync(resolve(SCHEMA_V2, `${typeName}.ts`))) return 'v2';
    if (existsSync(resolve(SCHEMA_ROOT, `${typeName}.ts`))) return 'root';
    return null;
}

/**
 * Parse a Rust type reference like `v2 :: ThreadStartParams` or
 * `# [serde (...)] Option < () >` into a TypeScript type name.
 *
 * Returns { tsType: string, module: 'v1' | 'v2' | 'root', isVoid: boolean }
 */
function parseRustType(raw) {
    const trimmed = raw.trim();

    // Option<()> — void params
    if (/Option\s*<\s*\(\s*\)\s*>/.test(trimmed)) {
        return { tsType: 'Record<string, never>', module: 'root', isVoid: true };
    }

    // v1::TypeName or v2::TypeName
    const modMatch = trimmed.match(/(?:v1|v2)\s*::\s*(\w+)/);
    if (modMatch) {
        const mod = trimmed.includes('v1') ? 'v1' : 'v2';
        return { tsType: modMatch[1], module: mod, isVoid: false };
    }

    // Bare type name (root-level, e.g. FuzzyFileSearchParams)
    const bareMatch = trimmed.match(/(\w+)\s*$/);
    if (bareMatch) {
        return { tsType: bareMatch[1], module: 'root', isVoid: false };
    }

    throw new Error(`Cannot parse Rust type: ${raw}`);
}

// ---------------------------------------------------------------------------
// Macro parsers
// ---------------------------------------------------------------------------

/**
 * Extract the full text of a macro invocation from the source.
 * Returns the text between the outer braces `{ ... }`.
 */
function extractMacroBody(source, macroName) {
    const idx = source.indexOf(`${macroName}!`);
    if (idx === -1) throw new Error(`Macro ${macroName} not found`);

    // Find the opening brace
    let start = source.indexOf('{', idx);
    if (start === -1) throw new Error(`No opening brace for ${macroName}`);

    // Match braces to find the end
    let depth = 1;
    let i = start + 1;
    while (i < source.length && depth > 0) {
        if (source[i] === '{') depth++;
        else if (source[i] === '}') depth--;
        i++;
    }

    return source.slice(start + 1, i - 1);
}

/**
 * Parse client_request_definitions! and server_request_definitions! entries.
 */
function parseRequestEntries(body) {
    const entries = [];
    const text = body.replace(/\n/g, ' ');

    const variantRe =
        /(?:#\s*\[[^\]]*\]\s*)*(\w+)\s*(?:=>\s*"([^"]*)")?\s*\{([^}]+)\}/g;

    let match;
    while ((match = variantRe.exec(text)) !== null) {
        const [, variantName, wireName, block] = match;
        if (!block.includes('params')) continue;

        const paramsMatch = block.match(
            /params\s*:\s*(.*?)\s*,\s*(?:inspect_params|response)/,
        );
        if (!paramsMatch) continue;

        const responseMatch = block.match(/response\s*:\s*(.*?)\s*,?\s*$/);
        if (!responseMatch) continue;

        const method = wireName || toCamelCase(variantName);
        const params = parseRustType(paramsMatch[1]);
        const response = parseRustType(responseMatch[1]);

        entries.push({ variantName, method, params, response });
    }

    return entries;
}

/**
 * Parse server_notification_definitions! entries.
 */
function parseNotificationEntries(body) {
    const entries = [];
    const text = body.replace(/\n/g, ' ');

    const re =
        /(?:#\s*\[serde\s*\(\s*rename\s*=\s*"([^"]*)"\s*\)\s*]\s*)?(?:#\s*\[strum[^\]]*\]\s*)?(?:#\s*\[[^\]]*\]\s*)*(\w+)\s*(?:=>\s*"([^"]*)")?\s*\(\s*(?:v[12]\s*::\s*)?(\w+)\s*\)/g;

    let match;
    while ((match = re.exec(text)) !== null) {
        const [, serdeRename, variantName, arrowWire, typeName] = match;
        const method = serdeRename || arrowWire || toCamelCase(variantName);
        entries.push({ variantName, method, tsType: typeName });
    }

    return entries;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

const source = readFileSync(COMMON_RS, 'utf-8');

// Parse all three macro invocations
const clientBody = extractMacroBody(source, 'client_request_definitions');
const allClientEntries = parseRequestEntries(clientBody);

const serverReqBody = extractMacroBody(source, 'server_request_definitions');
const allServerReqEntries = parseRequestEntries(serverReqBody);

const notifBody = extractMacroBody(source, 'server_notification_definitions');
const allNotifEntries = parseNotificationEntries(notifBody);

// Filter to entries whose types actually exist in the generated schema.
// Types behind #[experimental] may not be exported yet.
const skipped = [];

function typesExist(entry) {
    for (const field of ['params', 'response']) {
        const t = entry[field];
        if (t.isVoid) continue;
        const loc = findTypeLocation(t.tsType);
        if (!loc) {
            skipped.push(`${entry.method}: ${t.tsType}`);
            return false;
        }
        // Override module to match actual file location
        t.module = loc;
    }
    return true;
}

function notifTypeExists(entry) {
    const loc = findTypeLocation(entry.tsType);
    if (!loc) {
        skipped.push(`${entry.method}: ${entry.tsType}`);
        return false;
    }
    entry.module = loc;
    return true;
}

const clientEntries = allClientEntries.filter(typesExist);
const serverReqEntries = allServerReqEntries.filter(typesExist);
const notifEntries = allNotifEntries.filter(notifTypeExists);

// Collect all type imports needed, categorized by actual schema location
const v2Types = new Set();
const rootTypes = new Set();

function addType(tsType, module, isVoid) {
    if (isVoid) return;
    (module === 'v2' ? v2Types : rootTypes).add(tsType);
}

for (const e of clientEntries) {
    addType(e.params.tsType, e.params.module, e.params.isVoid);
    addType(e.response.tsType, e.response.module, e.response.isVoid);
}
for (const e of serverReqEntries) {
    addType(e.params.tsType, e.params.module, e.params.isVoid);
    addType(e.response.tsType, e.response.module, e.response.isVoid);
}
for (const e of notifEntries) {
    addType(e.tsType, e.module, false);
}

// Generate TypeScript
const lines = [];

lines.push(
    '// AUTO-GENERATED by scripts/generate-protocol-map.mjs — do not edit manually.',
);
lines.push(
    '// Source: runtime/codex-upstream/codex-rs/app-server-protocol/src/protocol/common.rs',
);
lines.push('');

// All types come from the same package — split into two import statements
// only when there are root-level types not already in v2
const allFromPackage = new Set([...v2Types, ...rootTypes]);
if (allFromPackage.size > 0) {
    const sorted = [...allFromPackage].sort();
    lines.push('import type {');
    for (const t of sorted) {
        lines.push(`    ${t},`);
    }
    lines.push("} from '@tjfontaine/codex-protocol-types';");
    lines.push('');
}

// ClientRequestMap
lines.push(
    '/** Maps client → server request method strings to their params and response types. */',
);
lines.push('export interface ClientRequestMap {');
for (const e of clientEntries) {
    const pType = e.params.isVoid ? 'Record<string, never>' : e.params.tsType;
    const rType = e.response.isVoid
        ? 'Record<string, never>'
        : e.response.tsType;
    lines.push(`    '${e.method}': { params: ${pType}; response: ${rType} };`);
}
lines.push('}');
lines.push('');

lines.push(
    '/** Union of all valid client → server request method strings. */',
);
lines.push('export type ClientMethod = keyof ClientRequestMap;');
lines.push('');

// ServerRequestMap
lines.push(
    '/** Maps server → client request method strings to their params and response types. */',
);
lines.push('export interface ServerRequestMap {');
for (const e of serverReqEntries) {
    const pType = e.params.isVoid ? 'Record<string, never>' : e.params.tsType;
    const rType = e.response.isVoid
        ? 'Record<string, never>'
        : e.response.tsType;
    lines.push(`    '${e.method}': { params: ${pType}; response: ${rType} };`);
}
lines.push('}');
lines.push('');

// ServerNotificationMap
lines.push(
    '/** Maps server → client notification method strings to their payload types. */',
);
lines.push('export interface ServerNotificationMap {');
for (const e of notifEntries) {
    lines.push(`    '${e.method}': ${e.tsType};`);
}
lines.push('}');
lines.push('');

writeFileSync(OUTPUT, lines.join('\n') + '\n');

console.log(`Generated ${OUTPUT}`);
console.log(
    `  ${clientEntries.length} client requests, ${serverReqEntries.length} server requests, ${notifEntries.length} notifications`,
);
if (skipped.length > 0) {
    console.log(`  Skipped ${skipped.length} entries (types not in schema):`);
    for (const s of skipped) {
        console.log(`    - ${s}`);
    }
}
