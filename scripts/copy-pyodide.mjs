#!/usr/bin/env node

/**
 * Copy Pyodide static assets to frontend/public/pyodide/
 *
 * Pyodide needs its WASM binary, stdlib packages, and lock file served
 * from a known URL prefix. We copy the essential files so Vite serves
 * them as static assets at /pyodide/.
 *
 * Source priority:
 *   1. Custom WasmFS Pyodide build from pyodide/ submodule (pyodide/dist/)
 *   2. npm fallback (local dev only — warns about missing OPFS behavior)
 *
 * In CI (detected via CI env var or REQUIRE_CUSTOM_PYODIDE=1), the script
 * refuses to fall back to npm and exits with an actionable error.
 */

import { cpSync, mkdirSync, rmSync, readdirSync, statSync, readFileSync, writeFileSync, existsSync } from 'fs';
import { resolve, dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { createRequire } from 'module';
import { createHash } from 'crypto';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = resolve(__dirname, '..');

const isCI = !!(process.env.CI || process.env.REQUIRE_CUSTOM_PYODIDE);

// Required files that must exist in the custom build for it to be valid.
const REQUIRED_FORK_FILES = [
    'pyodide.asm.wasm',
    'pyodide.asm.mjs',
    'pyodide.mjs',
    'python_stdlib.zip',
    'pyodide-lock.json',
];

// All files to copy from the source.
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
const REQUIRED_PACKAGES = ['micropip', 'packaging'];

// ==========================================================================
// Resolve Pyodide source
// ==========================================================================

const pyodideForkDist = resolve(root, 'pyodide', 'dist');

function validateForkBuild() {
    const missing = REQUIRED_FORK_FILES.filter(
        (f) => !existsSync(resolve(pyodideForkDist, f))
    );
    return missing;
}

let pyodideSrc;
const forkMissing = validateForkBuild();

if (forkMissing.length === 0) {
    pyodideSrc = pyodideForkDist;
    console.log(`Using custom Pyodide build from submodule: ${pyodideSrc}`);
} else if (isCI) {
    console.error('\n' + '='.repeat(70));
    console.error('ERROR: Custom Pyodide build is required in CI but is missing or incomplete.');
    console.error('Missing files in pyodide/dist/:');
    for (const f of forkMissing) {
        console.error(`  - ${f}`);
    }
    console.error('\nEnsure the "Build Pyodide dist" CI step ran successfully.');
    console.error('If the self-hosted runner lacks build prerequisites (emscripten, cmake),');
    console.error('fix the runner image rather than falling back to npm.');
    console.error('='.repeat(70) + '\n');
    process.exit(1);
} else {
    // Local dev fallback to npm — warn about behavior difference
    const require = createRequire(resolve(root, 'packages', 'wasm-python', 'package.json'));
    pyodideSrc = dirname(require.resolve('pyodide/package.json'));
    console.warn('\n⚠ WARNING: Using Pyodide from npm (custom fork build not found).');
    console.warn('  OPFS file-sharing behavior will NOT work with the npm build.');
    console.warn('  To use the custom fork: git submodule update --init pyodide && build pyodide/dist\n');
    console.log(`Using Pyodide from npm: ${pyodideSrc}`);
}

const pyodideDest = resolve(root, 'frontend', 'public', 'pyodide');

// ==========================================================================
// Clear stale artifacts before copying
// ==========================================================================

if (existsSync(pyodideDest)) {
    rmSync(pyodideDest, { recursive: true, force: true });
    console.log('Cleared stale frontend/public/pyodide/ directory');
}
mkdirSync(pyodideDest, { recursive: true });

// ==========================================================================
// Copy essential files
// ==========================================================================

let copiedCount = 0;

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

// ==========================================================================
// Log version info from lock file
// ==========================================================================

const lockPath = join(pyodideDest, 'pyodide-lock.json');
if (existsSync(lockPath)) {
    const lockData = JSON.parse(readFileSync(lockPath, 'utf8'));
    const pyodideVersion = lockData.info?.version;
    const pythonVersion = lockData.info?.python;

    if (pyodideVersion) {
        console.log(`\nPyodide lock version: ${pyodideVersion}`);
    }
    if (pythonVersion) {
        console.log(`Python version: ${pythonVersion}`);
    }

    // ======================================================================
    // Download required wheel packages from Pyodide CDN
    // ======================================================================

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
