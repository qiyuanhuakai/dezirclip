import { describe, it, expect, vi } from "vitest";
import { createFrameThrottle } from "./frameThrottle";

/** Runs queued callbacks on demand, the way a real frame boundary would. */
const makeFrames = () => {
  const queue: Array<() => void> = [];
  let next = 0;
  return {
    request: (cb: () => void) => {
      const handle = ++next;
      queue.push(() => {
        if (cb) cb();
      });
      return handle;
    },
    cancel: (handle: number) => {
      void handle;
    },
    runAll: () => {
      const pending = queue.splice(0, queue.length);
      for (const cb of pending) cb();
    },
    get depth() {
      return queue.length;
    },
  };
};

describe("createFrameThrottle", () => {
  // The whole point: a 1000 Hz mouse must not cost a thousand layout reads.
  it("collapses a burst into a single call with the last value", () => {
    const frames = makeFrames();
    const apply = vi.fn();
    const throttle = createFrameThrottle<number>(apply, frames.request, frames.cancel);

    for (let i = 0; i < 1000; i++) {
      throttle.schedule(i);
    }

    expect(apply).not.toHaveBeenCalled();
    frames.runAll();
    expect(apply).toHaveBeenCalledTimes(1);
    expect(apply).toHaveBeenCalledWith(999);
  });

  it("only ever queues one frame at a time", () => {
    const frames = makeFrames();
    const throttle = createFrameThrottle<number>(vi.fn(), frames.request, frames.cancel);

    for (let i = 0; i < 50; i++) {
      throttle.schedule(i);
      expect(frames.depth).toBe(1);
    }
  });

  it("keeps sampling across successive frames", () => {
    const frames = makeFrames();
    const apply = vi.fn();
    const throttle = createFrameThrottle<string>(apply, frames.request, frames.cancel);

    throttle.schedule("a");
    frames.runAll();
    throttle.schedule("b");
    frames.runAll();
    throttle.schedule("c");
    frames.runAll();

    expect(apply.mock.calls.map((c) => c[0])).toEqual(["a", "b", "c"]);
  });

  it("flushNow applies the queued value without waiting for a frame", () => {
    const frames = makeFrames();
    const apply = vi.fn();
    const throttle = createFrameThrottle<number>(apply, frames.request, frames.cancel);

    throttle.schedule(7);
    throttle.flushNow();

    expect(apply).toHaveBeenCalledTimes(1);
    expect(apply).toHaveBeenCalledWith(7);
  });

  it("flushNow on an empty queue is a no-op", () => {
    const frames = makeFrames();
    const apply = vi.fn();
    const throttle = createFrameThrottle<number>(apply, frames.request, frames.cancel);

    throttle.flushNow();

    expect(apply).not.toHaveBeenCalled();
  });

  // Leaving an item must not let a queued frame resurrect its anchor.
  it("cancel drops the queued value", () => {
    const frames = makeFrames();
    const apply = vi.fn();
    const throttle = createFrameThrottle<number>(apply, frames.request, frames.cancel);

    throttle.schedule(42);
    throttle.cancel();
    frames.runAll();

    expect(apply).not.toHaveBeenCalled();
  });

  it("stays usable after a cancel", () => {
    const frames = makeFrames();
    const apply = vi.fn();
    const throttle = createFrameThrottle<number>(apply, frames.request, frames.cancel);

    throttle.schedule(1);
    throttle.cancel();
    throttle.schedule(2);
    frames.runAll();

    expect(apply).toHaveBeenCalledTimes(1);
    expect(apply).toHaveBeenCalledWith(2);
  });
});
