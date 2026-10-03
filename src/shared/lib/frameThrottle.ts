/**
 * Coalesces a burst of calls into at most one per animation frame, keeping the
 * most recent argument.
 *
 * The pattern this exists for is a `mousemove` handler that has to read
 * geometry: a 1000 Hz mouse produces a thousand events a second, and every one
 * of them forced a synchronous layout via `getBoundingClientRect`. Sampling
 * once per frame is not a compromise on accuracy here — the browser cannot
 * change layout between two events that land in the same frame, and a value
 * read in the frame callback is measured after the newest pending layout
 * rather than before it.
 */
export const createFrameThrottle = <T>(
  apply: (value: T) => void,
  requestFrame: (cb: () => void) => number,
  cancelFrame: (handle: number) => void
) => {
  let latest: { value: T } | null = null;
  let handle: number | null = null;
  let active = false;

  const flush = () => {
    handle = null;
    active = false;
    const pending = latest;
    latest = null;
    if (pending) apply(pending.value);
  };

  return {
    /**
     * Queues `value` for the next frame. Repeated calls in the same frame
     * collapse to one, and the last value wins.
     */
    schedule(value: T) {
      latest = { value };
      if (active) return;
      active = true;
      handle = requestFrame(flush);
    },

    /**
     * Runs any queued value immediately and drops the pending frame. Used on
     * release, where the final position has to be accounted for even if the
     * next frame has not arrived yet.
     */
    flushNow() {
      if (handle !== null) {
        cancelFrame(handle);
        handle = null;
      }
      active = false;
      flush();
    },

    /** Drops the queued value without applying it. */
    cancel() {
      if (handle !== null) {
        cancelFrame(handle);
        handle = null;
      }
      active = false;
      latest = null;
    },
  };
};
