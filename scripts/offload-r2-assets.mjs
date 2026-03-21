#!/usr/bin/env node
/**
 * Upload all .wasm files from frontend/dist (and extra sources) to R2 CDN.
 *
 * All WASM assets are served from the R2 public bucket (cdn.edge-agent.dev)
 * instead of Workers static assets. This avoids the 25 MiB per-file limit
 * and keeps the Worker focused on compute.
 *
 * R2 keys are prefixed with `builds/{BUILD_ID}/` so each deploy gets a unique
 * namespace. Since the URL changes per deploy, all assets are cached immutably.
 * The Worker redirects .wasm requests to cdn.edge-agent.dev/builds/{buildId}/...
 *
 * Usage:
 *   node scripts/offload-r2-assets.mjs [--dry-run]
 *
 * Environment:
 *   CLOUDFLARE_API_TOKEN — wrangler auth
 *   R2_BUCKET            — bucket name (default: edge-agent-assets)
 *   BUILD_ID             — deploy identifier (default: git short SHA)
 */

import { readdirSync, statSync, unlinkSync, existsSync } from 'node:fs';
import { join, relative } from 'node:path';
import { execSync } from 'node:child_process';

const ROOT_DIR = join(import.meta.dirname, '..');
const DIST_DIR = join(ROOT_DIR, 'frontend', 'dist');
const R2_BUCKET = process.env.R2_BUCKET || 'edge-agent-assets';
const DRY_RUN = process.argv.includes('--dry-run');

// Build ID: explicit env var, or fall back to git short SHA
const BUILD_ID = process.env.BUILD_ID
    || execSync('git rev-parse --short HEAD', { encoding: 'utf-8' }).trim();

/** Recursively find all files in a directory */
function walkDir(dir) {
    const results = [];
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const full = join(dir, entry.name);
        if (entry.isDirectory()) {
            results.push(...walkDir(full));
        } else {
            results.push(full);
        }
    }
    return results;
}

/**
 * Collect all WASM assets to upload.
 * Returns array of { localPath, r2Path } where r2Path is the CDN-relative path
 * (before the builds/{buildId}/ prefix).
 */
function collectAssets() {
    const assets = [];

    // 1. All .wasm files from frontend/dist (Vite-bundled WASM components).
    //    Deleted from dist after upload — the deploy workflow also strips them
    //    because Workers static assets has a 25 MiB per-file limit.
    if (existsSync(DIST_DIR)) {
        for (const filePath of walkDir(DIST_DIR).filter(f => f.endsWith('.wasm'))) {
            assets.push({
                localPath: filePath,
                r2Path: relative(DIST_DIR, filePath),
                removeAfter: true,
            });
        }
    }

    // 2. stripe.wasm — built separately by the Go toolchain, lives in
    //    stripe-cli-wasm/ (not in dist). Kept in place after upload because
    //    it's a build output, not a dist artifact, and Moon may need it for
    //    cache validation.
    const stripeWasm = join(ROOT_DIR, 'stripe-cli-wasm', 'stripe.wasm');
    if (existsSync(stripeWasm)) {
        assets.push({
            localPath: stripeWasm,
            r2Path: 'wasm-stripe/stripe.wasm',
            removeAfter: false,
        });
    }

    return assets;
}

const assets = collectAssets();

if (assets.length === 0) {
    console.log('No WASM assets found. Nothing to upload.');
    process.exit(0);
}

console.log(`Build ID: ${BUILD_ID}`);
console.log(`Uploading ${assets.length} WASM asset(s) to R2 bucket "${R2_BUCKET}":\n`);

let totalBytes = 0;

for (const { localPath, r2Path, removeAfter } of assets) {
    const r2Key = `builds/${BUILD_ID}/${r2Path}`;
    const size = statSync(localPath).size;
    const sizeMiB = (size / (1024 * 1024)).toFixed(1);
    totalBytes += size;

    console.log(`  ${r2Key} (${sizeMiB} MiB)`);

    if (!DRY_RUN) {
        execSync(
            `npx wrangler r2 object put "${R2_BUCKET}/${r2Key}" --file "${localPath}" --content-type application/wasm --remote`,
            { stdio: 'inherit' },
        );
        if (removeAfter) {
            unlinkSync(localPath);
        }
    }
}

const totalMiB = (totalBytes / (1024 * 1024)).toFixed(1);
console.log(`\n${DRY_RUN ? '[dry-run] ' : ''}${assets.length} files (${totalMiB} MiB total) → R2 (builds/${BUILD_ID}/).`);
if (!DRY_RUN) {
    console.log('Removed dist .wasm files. stripe.wasm kept in place.');
}
