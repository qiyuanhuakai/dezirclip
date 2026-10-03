import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  createDragState,
  isDragHandleTarget,
  DRAGGING_CLASS,
  DRAG_SETTLE_MS,
} from "./dragDegradation";

const makeElement = () => {
  const classes = new Set<string>();
  return {
    classes,
    classList: {
      add: (c: string) => void classes.add(c),
      remove: (c: string) => void classes.delete(c),
    },
  };
};

describe("createDragState", () => {
  let element: ReturnType<typeof makeElement>;
  let now: number;
  let pending: Map<number, { deadline: number; fn: () => void }>;
  let nextHandle: number;

  const setTimer = (fn: () => void, ms: number) => {
    const handle = nextHandle++;
    pending.set(handle, { deadline: now + ms, fn });
    return handle;
  };
  const clearTimer = (handle: number) => void pending.delete(handle);

  /** Fire every timer whose deadline has passed, as the browser would. */
  const advance = (ms: number) => {
    now += ms;
    for (const [handle, entry] of [...pending]) {
      if (entry.deadline <= now) {
        pending.delete(handle);
        entry.fn();
      }
    }
  };

  beforeEach(() => {
    element = makeElement();
    now = 0;
    pending = new Map();
    nextHandle = 1;
  });

  it("adds the degradation class on the first frame of a drag", () => {
    const { actions } = createDragState(element, setTimer, clearTimer);
    expect(element.classes.has(DRAGGING_CLASS)).toBe(false);

    actions.enter();
    expect(element.classes.has(DRAGGING_CLASS)).toBe(true);
  });

  it("restores the effects one settle window after the drag stops", () => {
    const { actions } = createDragState(element, setTimer, clearTimer);
    actions.enter();

    advance(DRAG_SETTLE_MS);
    expect(element.classes.has(DRAGGING_CLASS)).toBe(false);
  });

  // The whole point of debouncing on Moved: a drag that runs for a couple of
  // seconds delivers hundreds of frames, and the class must not blink back on
  // between two of them.
  it("stays degraded across a long run of movement frames", () => {
    const { state, actions } = createDragState(element, setTimer, clearTimer);

    for (let frame = 0; frame < 500; frame++) {
      actions.hold();
      advance(16);
    }

    expect(element.classes.has(DRAGGING_CLASS)).toBe(true);
    expect(state.dragging).toBe(true);
    // One outstanding timer at a time, not one per frame.
    expect(pending.size).toBe(1);

    advance(DRAG_SETTLE_MS);
    expect(element.classes.has(DRAGGING_CLASS)).toBe(false);
  });

  it("does not re-add the class across a run of frames once entered", () => {
    const add = vi.spyOn(element.classList, "add");
    const remove = vi.spyOn(element.classList, "remove");
    const { actions } = createDragState(element, setTimer, clearTimer);

    for (let frame = 0; frame < 200; frame++) {
      actions.hold();
    }

    expect(add).toHaveBeenCalledTimes(1);
    expect(remove).not.toHaveBeenCalled();
  });

  // A click on the drag handle produces a pointerdown and no Moved event at
  // all. Without the settle timer the window would be stuck degraded.
  it("recovers on its own when a drag never produces a move", () => {
    const { actions } = createDragState(element, setTimer, clearTimer);
    actions.enter();

    advance(DRAG_SETTLE_MS);
    expect(element.classes.has(DRAGGING_CLASS)).toBe(false);
  });

  it("cancels a pending restore when the drag resumes", () => {
    const { state, actions } = createDragState(element, setTimer, clearTimer);

    actions.enter();
    advance(DRAG_SETTLE_MS - 1);
    actions.hold();
    advance(DRAG_SETTLE_MS - 1);

    expect(element.classes.has(DRAGGING_CLASS)).toBe(true);
    expect(state.timer).not.toBeNull();
  });

  it("reset clears immediately and drops any pending restore", () => {
    const { state, actions } = createDragState(element, setTimer, clearTimer);
    actions.enter();

    actions.reset();

    expect(element.classes.has(DRAGGING_CLASS)).toBe(false);
    expect(state.timer).toBeNull();
    expect(pending.size).toBe(0);
  });
});

describe("isDragHandleTarget", () => {
  let host: HTMLDivElement;
  let handle: HTMLDivElement;
  let plain: HTMLSpanElement;

  beforeEach(() => {
    document.body.innerHTML = "";
    handle = document.createElement("div");
    handle.setAttribute("data-tauri-drag-region", "");
    const child = document.createElement("span");
    handle.appendChild(child);
    plain = document.createElement("span");
    host = document.createElement("div");
    host.appendChild(handle);
    host.appendChild(plain);
    document.body.appendChild(host);
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("recognises the drag handle itself", () => {
    expect(isDragHandleTarget(handle)).toBe(true);
  });

  // The header's drag region wraps interactive-looking children, so the check
  // has to walk up rather than test the event target directly.
  it("recognises a descendant of the drag handle", () => {
    expect(isDragHandleTarget(handle.querySelector("span"))).toBe(true);
  });

  it("rejects anything outside a drag handle", () => {
    expect(isDragHandleTarget(plain)).toBe(false);
  });

  it("rejects non-element targets", () => {
    expect(isDragHandleTarget(null)).toBe(false);
  });
});
