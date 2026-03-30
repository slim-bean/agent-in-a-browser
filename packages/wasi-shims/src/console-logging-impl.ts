/**
 * Console Logging Shim.
 *
 * Implements the host-side of `host:console/logging@0.1.0`.
 * Routes WASM logging calls to the browser console instead of stderr.
 */

/**
 * Log a message (console.log).
 * JCO maps `host:console/logging@0.1.0#log` to this function.
 */
export function log(msg: string): void {
    console.log(msg);
}

/**
 * Log a warning (console.warn).
 * JCO maps `host:console/logging@0.1.0#warn` to this function.
 */
export function warn(msg: string): void {
    console.warn(msg);
}

/**
 * Log an error (console.error).
 * JCO maps `host:console/logging@0.1.0#error` to this function.
 */
export function error(msg: string): void {
    console.error(msg);
}
