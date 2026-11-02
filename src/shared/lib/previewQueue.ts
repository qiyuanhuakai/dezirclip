// At most two original images per window may be decoding for a list preview.
// Unmounting a row removes its queued work before it can reach the Host.
export function createPreviewQueue(limit: number) {
  let active = 0;
  const waiting = new Set<() => void>();
  return function schedule<T>(signal: AbortSignal, read: () => Promise<T>): Promise<T> {
    return new Promise((resolve, reject) => {
      const cancel = () => { waiting.delete(start); reject(new Error("PreviewCancelled")); };
      const start = () => {
        waiting.delete(start);
        signal.removeEventListener("abort", cancel);
        active++;
        Promise.resolve().then(() => {
          signal.throwIfAborted();
          return read();
        }).finally(() => {
          active--;
          waiting.values().next().value?.();
        }).then(resolve, reject);
      };
      if (signal.aborted) { cancel(); return; }
      if (active < limit) start();
      else { waiting.add(start); signal.addEventListener("abort", cancel, { once: true }); }
    });
  };
}

export const schedulePreview = createPreviewQueue(2);
