/**
 * @tjfontaine/wasm-git
 *
 * Git CLI module for the WASM shell.
 * Provides the 'git' command backed by a Go (go-git) → wasip1 → wasip2 compiled binary.
 *
 * NOTE: This package exports only metadata. The loader is provided by
 * the consuming application (e.g., frontend/lazy-modules.ts) to avoid
 * Rollup trying to resolve the dynamic WASM imports at build time.
 */

import type { ModuleMetadata } from '@tjfontaine/wasm-loader';

/**
 * Module metadata for git CLI
 */
export const metadata: ModuleMetadata = {
    name: 'git-module',
    commands: [
        { name: 'git', mode: 'buffered' },
    ],
};

export default metadata;
