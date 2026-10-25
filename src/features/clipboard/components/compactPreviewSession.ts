/** Serializes native window changes while cancelling obsolete hover requests immediately. */
export function createPreviewSessionQueue<T>() {
    let sequence = 0;
    let active: { id: number; owner: object; value: T; valid: () => boolean } | null = null;
    let tail = Promise.resolve();
    const get = (id: number) => active?.id === id && active.valid() ? active.value : null;
    const enqueue = (action: () => Promise<void>) => {
        const result = tail.then(action);
        tail = result.catch(() => undefined);
        return result;
    };
    return {
        begin(owner: object, value: T, valid: () => boolean) {
            active = { id: ++sequence, owner, value, valid };
            return sequence;
        },
        get,
        owns(owner: object) { return active?.owner === owner; },
        current() { return active && get(active.id) ? active.id : null; },
        run(id: number, steps: Array<(value: T) => Promise<void>>) {
            return enqueue(async () => {
                for (const step of steps) {
                    const value = get(id);
                    if (value === null) return;
                    await step(value);
                }
            });
        },
        cancel(cleanup: () => Promise<void>, owner?: object) {
            if (owner && active?.owner !== owner) return Promise.resolve();
            active = null;
            return enqueue(cleanup);
        }
    };
}
