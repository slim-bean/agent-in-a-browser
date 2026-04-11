#!/usr/bin/env node

/**
 * Copy Pyodide static assets to frontend/public/pyodide/
 *
 * Pyodide needs its WASM binary, stdlib packages, and lock file served
 * from a known URL prefix. We copy the essential files from node_modules
 * so Vite serves them as static assets at /pyodide/.
 *
 * Additionally, we download micropip and its dependency (packaging) from
 * the Pyodide CDN so that `loadPackage('micropip')` works offline without
 * needing a runtime fetch to cdn.jsdelivr.net.
 */

import { cpSync, mkdirSync, readdirSync, statSync, readFileSync, writeFileSync, existsSync } from 'fs';
import { resolve, dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { createRequire } from 'module';
import { createHash } from 'crypto';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = resolve(__dirname, '..');

// Use our custom WasmFS Pyodide build from the submodule if available,
// otherwise fall back to the npm package.
const pyodideForkDist = resolve(root, 'pyodide', 'dist');
const hasForkBuild = existsSync(resolve(pyodideForkDist, 'pyodide.asm.wasm'));

let pyodideSrc;
if (hasForkBuild) {
    pyodideSrc = pyodideForkDist;
    console.log(`Using custom Pyodide build from submodule: ${pyodideSrc}`);
} else {
    // Fall back to npm package
    const require = createRequire(resolve(root, 'packages', 'wasm-python', 'package.json'));
    pyodideSrc = dirname(require.resolve('pyodide/package.json'));
    console.log(`Using Pyodide from npm: ${pyodideSrc}`);
}
const pyodideDest = resolve(root, 'frontend', 'public', 'pyodide');

// Essential Pyodide files to copy (skip large optional packages)
const ESSENTIAL_PATTERNS = [
    'pyodide.asm.wasm',
    'pyodide.asm.js',
    'pyodide.asm.mjs',
    'pyodide_py.tar',
    'pyodide-lock.json',
    'repodata.json',
    'python_stdlib.zip',
    'pyodide.mjs',
    'ffi.mjs',
    'package.json',
];

// Packages to download from the Pyodide CDN so they're available locally.
// Without these, loadPackage('micropip') fails in worker/sandboxed contexts
// where the runtime CDN fetch is blocked.
const REQUIRED_PACKAGES = ['micropip', 'packaging'];

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

// Also copy any .zip files for built-in packages
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

// Download required wheel packages from Pyodide CDN
const lockPath = join(pyodideDest, 'pyodide-lock.json');
if (existsSync(lockPath)) {
    const lockData = JSON.parse(readFileSync(lockPath, 'utf8'));
    const pyodideVersion = lockData.info?.version;

    if (pyodideVersion) {
        // Dev builds (e.g. "0.30.0.dev0") are published under /pyodide/dev/full/,
        // while release builds use /pyodide/v{version}/full/.
        const isDevBuild = pyodideVersion.includes('dev') || pyodideVersion.includes('alpha') || pyodideVersion.includes('beta');
        const cdnBase = isDevBuild
            ? `https://cdn.jsdelivr.net/pyodide/dev/full`
            : `https://cdn.jsdelivr.net/pyodide/v${pyodideVersion}/full`;

        for (const pkgName of REQUIRED_PACKAGES) {
            const pkgInfo = lockData.packages?.[pkgName];
            if (!pkgInfo?.file_name) {
                console.log(`  Skipped ${pkgName} (not in lock file)`);
                continue;
            }

            const destPath = join(pyodideDest, pkgInfo.file_name);

            // Skip if already downloaded and hash matches
            if (existsSync(destPath)) {
                const existing = readFileSync(destPath);
                const hash = createHash('sha256').update(existing).digest('hex');
                if (hash === pkgInfo.sha256) {
                    console.log(`  ${pkgInfo.file_name} already present (hash OK)`);
                    copiedCount++;
                    continue;
                }
            }

            const url = `${cdnBase}/${pkgInfo.file_name}`;
            console.log(`  Downloading ${pkgInfo.file_name} from CDN...`);

            try {
                const response = await fetch(url);
                if (!response.ok) {
                    console.warn(`  Failed to download ${pkgInfo.file_name}: HTTP ${response.status}`);
                    continue;
                }
                const buffer = Buffer.from(await response.arrayBuffer());

                // Verify SHA256
                const hash = createHash('sha256').update(buffer).digest('hex');
                if (pkgInfo.sha256 && hash !== pkgInfo.sha256) {
                    console.warn(`  SHA256 mismatch for ${pkgInfo.file_name}: expected ${pkgInfo.sha256}, got ${hash}`);
                    continue;
                }

                writeFileSync(destPath, buffer);
                console.log(`  Downloaded ${pkgInfo.file_name} (${(buffer.length / 1024).toFixed(0)}KB, hash OK)`);
                copiedCount++;
            } catch (err) {
                console.warn('  Failed to download %s: %s', pkgInfo.file_name, err.message);
            }
        }
    }
}

console.log(`\nCopied ${copiedCount} Pyodide files to frontend/public/pyodide/`);
