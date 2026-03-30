/**
 * Resource Registry — globalThis singleton tracking every live WASI resource.
 *
 * Uses Symbol.for() to ensure a single registry instance across all module loads,
 * consistent with the Pollable/Descriptor singleton patterns used elsewhere.
 */

const REGISTRY_KEY = Symbol.for('wasi:debug/resource-registry');

export type ResourceType =
    | 'Pollable'
    | 'InputStream'
    | 'OutputStream'
    | 'Descriptor'
    | 'FutureIncomingResponse';

export interface ResourceEntry {
    id: number;
    type: ResourceType;
    subtype: string;
    createdAt: number;       // performance.now()
    lastActivity: number;    // performance.now()
    disposed: false | number; // false while live, timestamp when disposed
    meta: Record<string, string>;
}

class ResourceRegistryImpl {
    entries = new Map<number, ResourceEntry>();
    nextId = 1;

    /**
     * Register a new resource. Returns the assigned id.
     */
    register(type: ResourceType, subtype: string, meta: Record<string, string> = {}): number {
        const id = this.nextId++;
        const now = performance.now();
        this.entries.set(id, {
            id,
            type,
            subtype,
            createdAt: now,
            lastActivity: now,
            disposed: false,
            meta,
        });
        return id;
    }

    /**
     * Touch lastActivity timestamp for a resource.
     */
    activity(id: number): void {
        const entry = this.entries.get(id);
        if (entry) {
            entry.lastActivity = performance.now();
        }
    }

    /**
     * Mark a resource as disposed. Removes it from the map after a short delay
     * so it can still be inspected briefly after disposal.
     */
    deregister(id: number): void {
        const entry = this.entries.get(id);
        if (entry) {
            entry.disposed = performance.now();
            // Remove after 10s so post-mortem snapshots can still see it
            setTimeout(() => {
                this.entries.delete(id);
            }, 10_000);
        }
    }

    /**
     * Return all live (not disposed) entries.
     */
    snapshot(): ResourceEntry[] {
        const result: ResourceEntry[] = [];
        for (const entry of this.entries.values()) {
            if (entry.disposed === false) {
                result.push(entry);
            }
        }
        return result;
    }

    /**
     * Return all live FutureIncomingResponse entries.
     */
    httpInFlight(): ResourceEntry[] {
        return this.snapshot().filter(e => e.type === 'FutureIncomingResponse');
    }
}

// Singleton registration via Symbol.for — same pattern as Pollable / Descriptor
if (!(globalThis as Record<symbol, unknown>)[REGISTRY_KEY]) {
    (globalThis as Record<symbol, unknown>)[REGISTRY_KEY] = new ResourceRegistryImpl();
}

export const resourceRegistry = (globalThis as Record<symbol, unknown>)[REGISTRY_KEY] as ResourceRegistryImpl;
