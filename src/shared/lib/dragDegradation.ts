/**
 * Debounced drag-state machine, kept apart from the hook so the timing rules
 * can be tested without a DOM or a Tauri window.
 *
 * The class that disables the theme's backdrop filters has to be present for
 * the whole drag and gone shortly after, and `Moved` arrives once per frame
 * while the window travels. Feeding every one of those frames into a state
 * machine that answers "should the degraded state be on right now?" is what
 * lets the React side stay completely out of the loop.
 */

/** Class the CSS degradation rules key off. */
export const DRAGGING_CLASS = "is-dragging";

/**
 * How long the window must sit still before the effects come back. Long enough
 * that a drag with a brief pause in the middle does not restore the blur and
 * immediately re-strip it, short enough that the window looks normal again
 * before the user has looked away from it.
 */
export const DRAG_SETTLE_MS = 180;

export type DragState = {
  /** True while the window is being dragged, plus the settle window after. */
  dragging: boolean;
  /** Handle of the pending settle timer, or null when none is outstanding. */
  timer: ReturnType<typeof setTimeout> | null;
};

export type DragActions = {
  /** Add the class and start the settle clock. */
  enter: () => void;
  /** Push the settle clock back — called once per `Moved` frame. */
  hold: () => void;
  /** Remove the class immediately, cancelling any pending restore. */
  reset: () => void;
};

/**
 * Wires a drag state machine to a class on `element` and a timer source.
 *
 * `enter` is deliberately idempotent: pointerdown and the first `Moved` both
 * mean "a drag started", and a drag that never produces a single `Moved`
 * event (a click on the drag region) still has to come back on its own.
 */
export const createDragState = (
  element: { classList: { add: (c: string) => void; remove: (c: string) => void } },
  setTimer: (fn: () => void, ms: number) => ReturnType<typeof setTimeout>,
  clearTimer: (handle: ReturnType<typeof setTimeout>) => void,
  settleMs: number = DRAG_SETTLE_MS
): { state: DragState; actions: DragActions } => {
  const state: DragState = { dragging: false, timer: null };

  const cancelPending = () => {
    if (state.timer !== null) {
      clearTimer(state.timer);
      state.timer = null;
    }
  };

  const exit = () => {
    cancelPending();
    state.dragging = false;
    element.classList.remove(DRAGGING_CLASS);
  };

  const hold = () => {
    cancelPending();
    if (!state.dragging) {
      state.dragging = true;
      element.classList.add(DRAGGING_CLASS);
    }
    state.timer = setTimer(() => {
      state.timer = null;
      state.dragging = false;
      element.classList.remove(DRAGGING_CLASS);
    }, settleMs);
  };

  return {
    state,
    actions: {
      enter: hold,
      hold,
      reset: exit,
    },
  };
};

/**
 * A pointerdown on something the user can operate is a click, not a drag, even
 * when it lands inside a drag region. The header is one whole drag region and the
 * panel buttons live inside it, so without this check every click on a header
 * button entered the degraded state: the theme's blur layers came off for the
 * settle window and the whole stack was recomputed when they went back on, twice
 * per panel switch, for a drag that never happened.
 */
const INTERACTIVE = "button, input, textarea, select, a, [role='button'], [contenteditable='true']";

/** True when a pointerdown landed on a native window-drag handle. */
export const isDragHandleTarget = (target: EventTarget | null): boolean => {
  if (!(target instanceof Element)) return false;
  if (target.closest(INTERACTIVE)) return false;
  return target.closest("[data-tauri-drag-region]") !== null;
};
