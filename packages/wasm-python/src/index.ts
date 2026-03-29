/**
 * @tjfontaine/wasm-python
 *
 * Python runtime module for the WASM shell (via Pyodide).
 * Provides 'python3', 'python', and 'pip' commands.
 *
 * NOTE: This package exports only metadata. The loader is provided by
 * the consuming application (e.g., frontend/lazy-modules.ts) to avoid
 * Rollup trying to resolve the dynamic imports at build time.
 */

import type { ModuleMetadata } from '@tjfontaine/wasm-loader';

/**
 * Module metadata for python (pyodide)
 */
export const metadata: ModuleMetadata = {
    name: 'pyodide-module',
    commands: [
        { name: 'python3', mode: 'buffered' },
        { name: 'python', mode: 'buffered' },
        { name: 'pip', mode: 'buffered' },
    ],
};

export default metadata;
