import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  createDragState,
  isDragHandleTarget,
} from "../lib/dragDegradation";

/**
 * Drops the theme's `backdrop-filter` layers for as long as the window is
 * being dragged, and puts them back one settle window after it stops.
 *
 * Dragging is the one interaction in this app that is genuinely per-frame:
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

    document.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("blur", onWindowBlur);

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
      window.removeEventListener("blur", onWindowBlur);
      actions.reset();
      if (unlisten) unlisten();
    };
  }, []);
};
