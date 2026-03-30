/**
 * poll-impl.ts - Custom poll implementation for browser environment
 * 
 * Provides Pollable class and all subclasses in ONE file to ensure
 * they all extend from the same globalThis-registered base class.
 * 
 * CRITICAL: Uses globalThis singleton pattern. The Pollable class is
 * registered on globalThis FIRST, then all subclasses extend it.
 */

import { resourceRegistry } from './resource-registry.js';

// Key for the global Pollable singleton
const POLLABLE_KEY = Symbol.for('wasi:io/poll.Pollable');

// Symbol marker for cross-bundle instanceof replacement
const POLLABLE_MARKER = Symbol.for('wasi:io/poll@0.2.9#Pollable');

/**
 * Base Pollable class compatible with JCO expectations.
 * Has Symbol marker for cross-bundle instanceof replacement.
 */
class PollableImpl {
    _registryId: number;

    constructor() {
        // Symbol marker for patched instanceof checks
        // Using Object.defineProperty to avoid TS index signature issues
        Object.defineProperty(this, POLLABLE_MARKER, { value: true, enumerable: false });
        this._registryId = resourceRegistry.register('Pollable', 'PollableImpl', {});
    }

    ready(): boolean {
        return true;
    }

    block(): void {
        // Override in subclasses
    }

    [Symbol.dispose](): void {
        resourceRegistry.deregister(this._registryId);
    }
}

// Register Pollable singleton on globalThis if not already set
if (!(globalThis as Record<symbol, unknown>)[POLLABLE_KEY]) {
    (globalThis as Record<symbol, unknown>)[POLLABLE_KEY] = PollableImpl;
}

// ALWAYS read from globalThis - this is the class all subclasses extend
const Pollable = (globalThis as Record<symbol, unknown>)[POLLABLE_KEY] as typeof PollableImpl;
type Pollable = InstanceType<typeof Pollable>;

/**
 * ReadyPollable - always ready immediately.
 * Used for streams that are synchronously readable.
 */
class ReadyPollable extends Pollable {
    constructor() {
        super();
        resourceRegistry.deregister(this._registryId); // Deregister base registration
        this._registryId = resourceRegistry.register('Pollable', 'ReadyPollable', {});
    }

    override ready(): boolean {
        return true;
    }

    override block(): void {
        resourceRegistry.activity(this._registryId);
        // Already ready, nothing to block on
    }
}

/**
 * DurationPollable - becomes ready after a duration elapses.
 */
class DurationPollable extends Pollable {
    #targetTime: bigint;

    constructor(durationNanos: bigint | number) {
        super();
        resourceRegistry.deregister(this._registryId); // Deregister base registration
        this._registryId = resourceRegistry.register('Pollable', 'DurationPollable', {
            durationNanos: String(durationNanos),
        });
        // performance.now() is in milliseconds, convert to nanoseconds
        const nowNanos = BigInt(Math.floor(performance.now() * 1e6));
        this.#targetTime = nowNanos + BigInt(durationNanos);
    }

    override ready(): boolean {
        const nowNanos = BigInt(Math.floor(performance.now() * 1e6));
        return nowNanos >= this.#targetTime;
    }

    override block(): Promise<void> {
        resourceRegistry.activity(this._registryId);
        // Always return a Promise to trigger JSPI suspension — even if the
        // timer already expired. This yields to the JS event loop, allowing
        // console messages, DOM rendering, and input to be processed.
        // Without this, WASM runs continuously and blocks the main thread.
        const remainingMs = this.ready() ? 0 : Number(this.#targetTime - BigInt(Math.floor(performance.now() * 1e6))) / 1e6;
        // Signal the JS watchdog that the cooperative scheduler is yielding.
        // This proves the WASM event loop is alive even when stdin/stderr are idle.
        if (typeof (globalThis as any).__wasmYieldActivity === 'function') {
            (globalThis as any).__wasmYieldActivity();
        }
        return new Promise(resolve => setTimeout(resolve, Math.max(0, Math.ceil(remainingMs))));
    }
}

/**
 * InstantPollable - becomes ready at a specific instant.
 */
class InstantPollable extends Pollable {
    #targetTime: bigint;

    constructor(instantNanos: bigint | number) {
        super();
        resourceRegistry.deregister(this._registryId); // Deregister base registration
        this._registryId = resourceRegistry.register('Pollable', 'InstantPollable', {
            instantNanos: String(instantNanos),
        });
        this.#targetTime = BigInt(instantNanos);
    }

    override ready(): boolean {
        const nowNanos = BigInt(Math.floor(performance.now() * 1e6));
        return nowNanos >= this.#targetTime;
    }

    override block(): void {
        resourceRegistry.activity(this._registryId);
        while (!this.ready()) {
            // Busy wait
        }
    }
}

/**
 * Poll function matching WASI io.poll signature.
 */
export async function poll(list: Pollable[]): Promise<Uint32Array> {
    // Check if any pollable is already ready
    const readyIndices: number[] = [];
    for (let i = 0; i < list.length; i++) {
        const pollable = list[i];
        if (pollable.ready && typeof pollable.ready === 'function' && pollable.ready()) {
            readyIndices.push(i);
        }
    }

    if (readyIndices.length > 0) {
        return new Uint32Array(readyIndices);
    }

    // None ready - race all pollables with block() methods
    const blockPromises: Promise<number>[] = [];

    for (let i = 0; i < list.length; i++) {
        const pollable = list[i];
        if (pollable.block && typeof pollable.block === 'function') {
            blockPromises.push(
                Promise.resolve(pollable.block()).then(() => i)
            );
        }
    }

    if (blockPromises.length === 0) {
        await new Promise(resolve => setTimeout(resolve, 1));
        return new Uint32Array([list.length - 1]);
    }

    const readyIndex = await Promise.race(blockPromises);

    const allReady: number[] = [];
    for (let i = 0; i < list.length; i++) {
        const pollable = list[i];
        if (pollable.ready && pollable.ready()) {
            allReady.push(i);
        }
    }

    return new Uint32Array(allReady.length > 0 ? allReady : [readyIndex]);
}

/**
 * pollList - alternative signature for poll
 */
export function pollList(list: Pollable[]): Promise<Uint32Array> {
    return poll(list);
}

/**
 * pollOne - poll a single pollable
 */
export async function pollOne(pollable: Pollable): Promise<void> {
    if (pollable.ready && pollable.ready()) {
        return;
    }
    if (pollable.block) {
        await Promise.resolve(pollable.block());
    }
}

// Export all classes
export { Pollable, ReadyPollable, DurationPollable, InstantPollable };
