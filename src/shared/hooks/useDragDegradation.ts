import { useEffect } from "react";
import { getCurrentWindow } from "../lib/desktopWindow";
import {
  createDragState,
  isDragHandleTarget,
} from "../lib/dragDegradation";

/**
 * Drops the theme's `backdrop-filter` layers for as long as the window is
 * moving, resizing or scrolling, and puts them back once the viewport settles.
 *
 * These interactions invalidate the sampled backdrop on successive frames:
 * every pixel the window moves invalidates the whole painted area behind it,
 * so all 137 blur layers across the themes get recomputed from scratch on
 * each frame. Nothing about the drag depends on those layers being live, and
 * recomputing them is the most expensive thing happening on screen at the
 * exact moment the user is moving the window fastest.
 *
 * Three signals, none of which does per-frame work beyond resetting a timer:
 * pointerdown on a native drag handle covers the first frame, `Moved` covers
 * every frame after it, and the settle timer guarantees the effects come back
 * even if the drag ends without a final event.
 *
 * Deliberately no React state: the class is written straight to the document
 * element so that toggling it cannot itself schedule a render.
 */
export const useDragDegradation = () => {
  useEffect(() => {
    const root = document.documentElement;
    const { actions } = createDragState(
      root,
      (fn, ms) => window.setTimeout(fn, ms),
      (handle) => window.clearTimeout(handle)
    );

    const onPointerDown = (e: PointerEvent) => {
      if (isDragHandleTarget(e.target)) actions.enter();
    };
    const onWindowBlur = () => actions.reset();
    const onListScroll = (event: Event) => {
      if (event.target instanceof HTMLElement && event.target.closest(".virtual-list-wrapper")) actions.hold();
    };

    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("scroll", onListScroll, { capture: true, passive: true });
    window.addEventListener("blur", onWindowBlur);
    window.addEventListener("resize", actions.hold);
    window.visualViewport?.addEventListener("resize", actions.hold);

    let disposed = false;
    let unlisten: (() => void) | null = null;
    getCurrentWindow()
      .onMoved(() => actions.hold())
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlisten = fn;
      })
      .catch(() => {
        // A platform without the Moved event still degrades on pointerdown and
        // restores on the settle timer; it just cannot stay degraded for a
        // drag that outlasts the timer.
      });

    return () => {
      disposed = true;
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("scroll", onListScroll, true);
      window.removeEventListener("blur", onWindowBlur);
      window.removeEventListener("resize", actions.hold);
      window.visualViewport?.removeEventListener("resize", actions.hold);
      actions.reset();
      if (unlisten) unlisten();
    };
  }, []);
};
