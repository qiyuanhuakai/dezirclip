/** Coalesce reloads and keep a stale snapshot from replacing newer live events. */
export function createSnapshotRefresh<T>(
  read: () => Promise<T>,
  publish: (snapshot: T) => void,
  onError: () => void,
) {
  let revision = 0;
  let cancelled = false;
  let pending: Promise<void> | null = null;

  const run = async () => {
    while (!cancelled) {
      const request = revision;
      let snapshot: T;
      try { snapshot = await read(); }
      catch {
        if (cancelled) return;
        if (request !== revision) continue;
        onError();
        return;
      }
      if (cancelled) return;
      if (request !== revision) continue;
      publish(snapshot);
      if (request === revision) return;
    }
  };

  return {
    refresh() {
      revision += 1;
      cancelled = false;
      pending ??= run().finally(() => { pending = null; });
      return pending;
    },
    invalidate() {
      if (pending) revision += 1;
    },
    cancel() {
      cancelled = true;
      revision += 1;
    },
  };
}
