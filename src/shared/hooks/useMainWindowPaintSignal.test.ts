import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { useMainWindowPaintSignal } from "./useMainWindowPaintSignal";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mockInvoke = vi.mocked(invoke);

/**
 * Frames on demand, so "which frame reports the paint" is a fact about the test
 * rather than about how fast jsdom happens to run. A real `requestAnimationFrame`
 * would let the double-rAF pass on its own and prove nothing about the ordering
 * the backend's timeout depends on.
 */
const makeFrames = () => {
  const queue: Array<{ handle: number; cb: FrameRequestCallback }> = [];
  const cancelled = new Set<number>();
  let next = 0;
  let ran = 0;
  return {
    request: (cb: FrameRequestCallback) => {
      const handle = ++next;
      queue.push({ handle, cb });
      return handle;
    },
    cancel: (handle: number) => {
      cancelled.add(handle);
    },
    /**
     * Run one frame boundary.
     *
     * A cancelled handle is dropped rather than run: that is what the real
     * `cancelAnimationFrame` does, and stubbing it any other way turns every
     * unmount assertion into a test of the hook's own `cancelled` flag with the
     * cancellation path removed.
     */
    flush: (count = 1) => {
      for (let i = 0; i < count; i++) {
        const due = queue.splice(0, queue.length);
        for (const { handle, cb } of due) {
          if (cancelled.has(handle)) continue;
          ran++;
          cb(0);
        }
      }
    },
    get depth() {
      return queue.length;
    },
    get ranCount() {
      return ran;
    },
    /**
     * Handles that were issued, have not run, and were not cancelled -- the
     * frames a real browser would still fire. Counted before `flush`, since
     * draining the queue empties it either way.
     */
    get stillLive() {
      return queue.filter(({ handle }) => !cancelled.has(handle)).length;
    },
  };
};

describe("useMainWindowPaintSignal", () => {
  let frames: ReturnType<typeof makeFrames>;

  beforeEach(() => {
    mockInvoke.mockReset();
    mockInvoke.mockResolvedValue(undefined);
    frames = makeFrames();
    vi.stubGlobal("requestAnimationFrame", frames.request);
    vi.stubGlobal("cancelAnimationFrame", frames.cancel);
  });

  it("says nothing until the settings have arrived", () => {
    // Reporting a paint before there is anything to paint is what puts an empty
    // window back on screen — the very thing the wait exists to prevent.
    renderHook(() => useMainWindowPaintSignal(false));
    frames.flush(3);

    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("reports only on the second frame, not the first", () => {
    // One frame is the paint being queued; the second is it being on screen.
    // Reporting on the first puts the backend's window up before the compositor
    // has the frame, which is the empty window all over again.
    const { rerender } = renderHook(
      ({ loaded }: { loaded: boolean }) => useMainWindowPaintSignal(loaded),
      { initialProps: { loaded: false } }
    );

    rerender({ loaded: true });
    frames.flush(1);
    expect(mockInvoke).not.toHaveBeenCalled();

    frames.flush(1);
    expect(mockInvoke).toHaveBeenCalledTimes(1);
    expect(mockInvoke).toHaveBeenCalledWith("notify_main_window_painted");
  });

  it("reports once no matter how many later frames pass", () => {
    renderHook(() => useMainWindowPaintSignal(true));
    frames.flush(2);
    frames.flush(5);

    expect(mockInvoke).toHaveBeenCalledTimes(1);
  });

  it("never reports after the component is gone", () => {
    // The idle destroyer tears the webview down without unmounting React, but a
    // settings reload or a route change does unmount it, and a signal fired
    // after that belongs to no window at all.
    const { unmount } = renderHook(() => useMainWindowPaintSignal(true));
    frames.flush(1);
    unmount();
    frames.flush(4);

    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("takes the pending frames off the queue when it is torn down", () => {
    // Asserting only that no signal went out cannot tell a cancelled frame from
    // the hook's own `cancelled` guard: with the guard alone, a frame that was
    // never cancelled still runs and simply declines to report. So assert the
    // mechanism -- no frame left live, and none of them run -- which holds only
    // if `cancelAnimationFrame` actually reached the handle.
    const { unmount } = renderHook(() => useMainWindowPaintSignal(true));
    expect(frames.stillLive).toBe(1);

    unmount();

    expect(frames.stillLive).toBe(0);
    frames.flush(4);
    expect(frames.ranCount).toBe(0);
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("cancels the inner frame too once the first one has run", () => {
    // Unmounting between the two frames is the narrow window the cleanup has to
    // cover: the outer handle is already spent, so only the inner one is live.
    // The cleanup cancels both unconditionally -- including `inner` while it is
    // still 0, which the spec ignores -- so this is really a check that the
    // second handle is reached at all.
    const { unmount } = renderHook(() => useMainWindowPaintSignal(true));
    frames.flush(1);
    expect(frames.ranCount).toBe(1);
    expect(frames.stillLive).toBe(1);

    unmount();

    expect(frames.stillLive).toBe(0);
    frames.flush(4);
    expect(frames.ranCount).toBe(1);
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("does not report when the settings never load", () => {
    renderHook(() => useMainWindowPaintSignal(false));
    frames.flush(8);

    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("survives the backend rejecting the command", () => {
    // The backend shows on its own deadline regardless, so a failed call is not
    // worth retrying — but an unhandled rejection here would surface as a console
    // error on every single rebuild.
    mockInvoke.mockRejectedValue(new Error("command not registered"));
    expect(() => {
      renderHook(() => useMainWindowPaintSignal(true));
      act(() => {
        frames.flush(2);
      });
    }).not.toThrow();

    expect(mockInvoke).toHaveBeenCalledTimes(1);
  });
});