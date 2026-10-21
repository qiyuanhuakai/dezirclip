/** Keep one hydration in flight and one latest id; a snapshot fills any gaps. */
export function createCapturedEntryReader<T extends { id: number }>(
  read: (id: number, hostInstanceId?: string) => Promise<T>,
  publish: (entry: T) => void,
  refresh: () => void,
) {
  let revision = 0;
  let disposed = false;
  let running = false;
  let pending: { id: number; revision: number; hostInstanceId?: string } | null = null;
  let refreshNeeded = false;

  const invalidate = () => {
    const hadWork = running || pending !== null;
    revision += 1;
    pending = null;
    refreshNeeded = false;
    return hadWork;
  };

  const run = async () => {
    if (running || disposed) return;
    running = true;
    while (pending !== null && !disposed) {
      const request = pending;
      pending = null;
      try {
        const entry = await (request.hostInstanceId === undefined
          ? read(request.id) : read(request.id, request.hostInstanceId));
        if (!disposed && request.revision === revision) {
          if (entry.id !== request.id) throw new Error("Captured entry id differs");
          publish(entry);
        }
      } catch {
        if (!disposed && request.revision === revision) refreshNeeded = true;
      }
    }
    running = false;
    if (!disposed && refreshNeeded) {
      refreshNeeded = false;
      refresh();
    }
  };

  return {
    capture(id: number, hostInstanceId?: string) {
      if (disposed) return;
      if (!Number.isSafeInteger(id) || id === 0) {
        invalidate();
        refresh();
        return;
      }
      revision += 1;
      if (running) refreshNeeded = true;
      pending = { id, revision, hostInstanceId };
      void run();
    },
    invalidate,
    dispose() {
      disposed = true;
      invalidate();
    },
  };
}
