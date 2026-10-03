import { useState, useEffect, useLayoutEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useBackendAppearance } from "../../../shared/hooks/useBackendAppearance";
import "./RegionSelectWindow.css";

type Selection = {
  startX: number;
  startY: number;
  endX: number;
  endY: number;
};

type CaptureRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

type RegionSelectWindowProps = {
  onSelect?: (result: { x: number; y: number; width: number; height: number }) => void;
  onCancel?: () => void;
};

const MIN_SELECTION_SIZE = 10;
const CAPTURE_SETTLE_MS = 100;

const hideRegionSelectWindow = async () => {
  await getCurrentWindow().setFocusable(false);
  await getCurrentWindow().hide();
};

/**
 * A cancelled selection has no capture to make, and the overlay is fullscreen —
 * while it sits hidden it still holds a whole display's worth of compositor
 * surface. Handing the hide to the backend lets it drop the window's memory
 * target too, which is what gives the memory back. The success path skips this
 * because `capture_region` releases the selector once it has replied.
 */
const parkRegionSelectWindow = async () => {
  await invoke("hide_region_select").catch(() => undefined);
};

const normalizeRect = (sel: Selection) => {
  const x = Math.min(sel.startX, sel.endX);
  const y = Math.min(sel.startY, sel.endY);
  const width = Math.abs(sel.endX - sel.startX);
  const height = Math.abs(sel.endY - sel.startY);
  return { x, y, width, height };
};

const meetsMinimum = (rect: CaptureRect) =>
  rect.width >= MIN_SELECTION_SIZE && rect.height >= MIN_SELECTION_SIZE;

export const toPhysicalCaptureRect = (
  rect: CaptureRect,
  origin: { x: number; y: number },
  scale: number
): CaptureRect => ({
  x: Math.round(origin.x + rect.x * scale),
  y: Math.round(origin.y + rect.y * scale),
  width: Math.round(rect.width * scale),
  height: Math.round(rect.height * scale),
});

const RegionSelectWindow = ({ onSelect, onCancel }: RegionSelectWindowProps) => {
  const [selection, setSelection] = useState<Selection | null>(null);
  const [dragging, setDragging] = useState(false);
  const [boxVisible, setBoxVisible] = useState(false);

  const boxRef = useRef<HTMLDivElement | null>(null);
  const dimsRef = useRef<HTMLDivElement | null>(null);
  /**
   * The drag is the one genuinely per-frame interaction in this window, and a
   * 1000 Hz mouse delivers a thousand `mousemove` events a second. Routing each
   * one through setSelection meant a thousand full re-renders of this overlay
   * a second, each rebuilding the selection object, the normalised rect and the
   * inline style. The live geometry now lives in refs and reaches the screen
   * as direct style writes inside a single animation frame, so React is only
   * involved at the transitions it actually owns: the box crossing the minimum
   * size, and the capture finishing. That is at most two renders per drag.
   */
  const live = useRef<Selection | null>(null);
  const frameRef = useRef<number | null>(null);
  const framePending = useRef(false);
  const pendingRef = useRef<{ x: number; y: number } | null>(null);
  const visibleRef = useRef(false);

  // The capture overlay is usually opened straight from a hotkey, long before
  // the main window has ever rendered, so theme and colour mode come from the
  // backend rather than from the main window's local state.
  useBackendAppearance();

  useEffect(() => {
    document.body.classList.add("region-select");
  }, []);

  const paint = useCallback((sel: Selection) => {
    const rect = normalizeRect(sel);
    const box = boxRef.current;
    if (box) {
      box.style.left = `${rect.x}px`;
      box.style.top = `${rect.y}px`;
      box.style.width = `${rect.width}px`;
      box.style.height = `${rect.height}px`;
    }
    const dims = dimsRef.current;
    if (dims) {
      dims.textContent = `${rect.width} × ${rect.height}`;
    }
    // Only this one transition is worth a render: the box appearing once the
    // rectangle is big enough. Everything below the threshold stays unrendered
    // exactly as before, so the minimum-size behaviour is unchanged.
    if (visibleRef.current !== meetsMinimum(rect)) {
      visibleRef.current = meetsMinimum(rect);
      setBoxVisible(visibleRef.current);
    }
  }, []);

  const cancelFrame = useCallback(() => {
    if (frameRef.current !== null) {
      cancelAnimationFrame(frameRef.current);
      frameRef.current = null;
    }
    framePending.current = false;
  }, []);

  const schedulePaint = useCallback(() => {
    if (framePending.current) return;
    // The guard is raised before requesting the frame, not after: storing the
    // handle in the same statement would let a callback that runs first clear
    // the handle and leave a stale one behind, re-arming the guard forever.
    framePending.current = true;
    frameRef.current = requestAnimationFrame(() => {
      framePending.current = false;
      frameRef.current = null;
      const sel = live.current;
      const point = pendingRef.current;
      if (!sel || !point) return;
      live.current = { ...sel, endX: point.x, endY: point.y };
      paint(live.current);
    });
  }, [paint]);

  useEffect(() => cancelFrame, [cancelFrame]);

  // The box only exists once the rectangle clears the minimum size, which is
  // the same frame that first tries to write into it. Replaying the current
  // geometry on mount is what makes the very first visible box carry the right
  // position and dimensions instead of a frame of empty content.
  useLayoutEffect(() => {
    if (boxVisible && live.current) {
      paint(live.current);
    }
  }, [boxVisible, paint]);

  const clearSelection = useCallback(() => {
    cancelFrame();
    live.current = null;
    pendingRef.current = null;
    visibleRef.current = false;
    setBoxVisible(false);
    setSelection(null);
  }, [cancelFrame]);

  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      const start = {
        startX: e.clientX,
        startY: e.clientY,
        endX: e.clientX,
        endY: e.clientY,
      };
      live.current = start;
      pendingRef.current = null;
      visibleRef.current = false;
      setBoxVisible(false);
      setSelection(start);
      setDragging(true);
    },
    []
  );

  const handleMouseMove = useCallback(
    (e: React.MouseEvent) => {
      if (!dragging) return;
      pendingRef.current = { x: e.clientX, y: e.clientY };
      schedulePaint();
    },
    [dragging, schedulePaint]
  );

  const handleMouseUp = useCallback(async () => {
    if (!dragging) return;
    setDragging(false);
    // Flush the position queued for the next frame so a release between two
    // frames still captures the rectangle the user actually saw.
    cancelFrame();
    const last = live.current;
    const queued = pendingRef.current;
    if (last && queued) {
      live.current = { ...last, endX: queued.x, endY: queued.y };
    }
    const final = live.current;
    if (!final) {
      clearSelection();
      return;
    }

    const rect = normalizeRect(final);
    if (!meetsMinimum(rect)) {
      clearSelection();
      return;
    }

    try {
      const window = getCurrentWindow();
      const [origin, scale] = await Promise.all([
        window.outerPosition(),
        window.scaleFactor(),
      ]);
      const captureRect = toPhysicalCaptureRect(rect, origin, scale);
      clearSelection();
      await hideRegionSelectWindow();
      await new Promise((resolve) => globalThis.setTimeout(resolve, CAPTURE_SETTLE_MS));
      await invoke("capture_region", captureRect);
      onSelect?.(captureRect);
      return;
    } catch (err) {
      console.error("Failed to capture selected region", err);
    }

    clearSelection();
    await parkRegionSelectWindow();
  }, [dragging, onSelect, cancelFrame, clearSelection]);

  // ESC key handler
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        clearSelection();
        setDragging(false);
        onCancel?.();
        parkRegionSelectWindow().catch(() => undefined);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onCancel, clearSelection]);

  return (
    <div
      className="region-select-window"
      data-testid="region-select-overlay"
      onMouseDown={handleMouseDown}
      onMouseMove={handleMouseMove}
      onMouseUp={handleMouseUp}
    >
      {selection && boxVisible && (
        <div
          className="region-select-window__selection"
          data-testid="region-select-box"
          ref={boxRef}
        >
          <div
            className="region-select-window__dimensions"
            data-testid="region-select-dimensions"
            ref={dimsRef}
          />
        </div>
      )}
    </div>
  );
};

export default RegionSelectWindow;
