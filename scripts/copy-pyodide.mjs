#!/usr/bin/env node

/**
 * Copy Pyodide static assets to frontend/public/pyodide/
 *
 * Pyodide needs its WASM binary, stdlib packages, and lock file served
 * from a known URL prefix. We copy the essential files from node_modules
 * so Vite serves them as static assets at /pyodide/.
 */

import { cpSync, mkdirSync, readdirSync, statSync } from 'fs';
import { resolve, dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { createRequire } from 'module';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = resolve(__dirname, '..');

// Resolve pyodide package location via Node's module resolution
// (pnpm hoists packages into .pnpm/, so direct path won't work)
const require = createRequire(resolve(root, 'packages', 'wasm-python', 'package.json'));
const pyodideSrc = dirname(require.resolve('pyodide/package.json'));
const pyodideDest = resolve(root, 'frontend', 'public', 'pyodide');

// Essential Pyodide files to copy (skip large optional packages)
const ESSENTIAL_PATTERNS = [
    'pyodide.asm.wasm',
    'pyodide.asm.js',
    'pyodide_py.tar',
    'pyodide-lock.json',
    'repodata.json',
    'python_stdlib.zip',
    'pyodide.mjs',
    'ffi.mjs',
    'package.json',
];

mkdirSync(pyodideDest, { recursive: true });

let copiedCount = 0;

// Copy essential files
for (const pattern of ESSENTIAL_PATTERNS) {
    const src = join(pyodideSrc, pattern);
    try {
        statSync(src);
        cpSync(src, join(pyodideDest, pattern));
        const size = statSync(src).size;
        console.log(`  Copied ${pattern} (${(size / 1024 / 1024).toFixed(1)}MB)`);
        copiedCount++;
    } catch {
        // File may not exist in all Pyodide versions
        console.log(`  Skipped ${pattern} (not found)`);
    }
}

// Also copy any .so files for built-in packages (micropip etc)
try {
    const entries = readdirSync(pyodideSrc);
    for (const entry of entries) {
        if (entry.endsWith('.zip') && entry !== 'python_stdlib.zip') {
            const src = join(pyodideSrc, entry);
            cpSync(src, join(pyodideDest, entry));
            const size = statSync(src).size;
            console.log(`  Copied ${entry} (${(size / 1024).toFixed(0)}KB)`);
            copiedCount++;
        }
    }
} catch {
    // Fine if no extra packages
}

console.log(`\nCopied ${copiedCount} Pyodide files to frontend/public/pyodide/`);
