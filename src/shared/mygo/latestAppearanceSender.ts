// One in-flight update and one replaceable snapshot keep native appearance in
// generation order without accumulating intermediate slider/theme changes.
export function createLatestAppearanceSender<T>(send: (value: T) => Promise<void>, report: (error: unknown) => void) {
  let pending: { value: T } | null = null;
  let active = false;
  let closed = false;
  const drain = async () => {
    active = true;
    try {
      while (!closed && pending) {
        const { value } = pending;
        pending = null;
        try { await send(value); } catch (error) { report(error); }
      }
    } finally { active = false; }
  };
  return {
    offer(value: T) {
      if (closed) return;
      pending = { value };
      if (!active) void drain();
    },
    close() { closed = true; pending = null; }
  };
}
