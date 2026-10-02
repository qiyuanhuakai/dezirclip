/**
 * Scroll observation for the clipboard list.
 *
 * A fling delivers scroll events faster than the display refreshes. Reporting
 * them through a React `onScroll` prop made every event pay for React's
 * synthetic dispatch plus a state update, which is the most-touched path in the
 * app. The two consumers of the offset want it differently:
 *
 *   - the wheel heuristic reads it to decide whether the list is already at the
 *     top, and has to be current on the very next event, so it is called
 *     synchronously for every event;
 *   - the scroll-to-top button only reacts when the offset crosses its
 *     threshold, so it is called at most once per frame.
 *
 * Both are driven from one passive native listener, so neither the dispatch nor
 * the frame bookkeeping goes through React. The "a frame is coming" flag is
 * raised before the scheduler is asked, because a scheduler is allowed to run
 * the callback before returning; claiming the handle afterwards would leave a
 * permanent handle behind and stall every later event.
 */

export type FrameScheduler = (callback: () => void) => number;
export type FrameCanceller = (handle: number) => void;

export interface ScrollCoalesceOptions {
    requestFrame?: FrameScheduler;
    cancelFrame?: FrameCanceller;
    readOffset?: (element: HTMLElement) => number;
}

export interface ScrollCoalescer {
    /** Bind to an element, replacing any previous binding. */
    attach(element: HTMLElement): void;
    /** Unbind and drop any frame that has not run yet. */
    detach(): void;
    /** Run the pending frame now. Returns whether there was one. */
    flush(): boolean;
    /** Whether a frame is currently pending. */
    readonly pending: boolean;
}

const defaultRequestFrame: FrameScheduler = (callback) => requestAnimationFrame(callback);
const defaultCancelFrame: FrameCanceller = (handle) => cancelAnimationFrame(handle);
const defaultReadOffset = (element: HTMLElement): number => element.scrollTop;

export const createScrollCoalescer = (
    onScroll: (offset: number) => void,
    onFrame: (offset: number) => void,
    options: ScrollCoalesceOptions = {}
): ScrollCoalescer => {
    const requestFrame = options.requestFrame ?? defaultRequestFrame;
    const cancelFrame = options.cancelFrame ?? defaultCancelFrame;
    const readOffset = options.readOffset ?? defaultReadOffset;

    let element: HTMLElement | null = null;
    let offset = 0;
    let scheduled = false;
    let handle: number | null = null;

    const release = () => {
        if (handle === null) return;
        cancelFrame(handle);
        handle = null;
    };

    const detach = () => {
        if (element) {
            element.removeEventListener("scroll", listener);
        }
        element = null;
        scheduled = false;
        release();
    };

    const runFrame = () => {
        scheduled = false;
        handle = null;
        onFrame(offset);
    };

    const listener = () => {
        if (!element) return;
        offset = readOffset(element);
        onScroll(offset);
        if (scheduled) return;
        scheduled = true;
        const id = requestFrame(runFrame);
        if (scheduled) handle = id;
    };

    return {
        attach(next: HTMLElement) {
            if (element === next) return;
            detach();
            element = next;
            offset = readOffset(next);
            next.addEventListener("scroll", listener, { passive: true });
        },
        detach,
        flush() {
            if (!scheduled) return false;
            release();
            runFrame();
            return true;
        },
        get pending() {
            return scheduled;
        }
    };
};
