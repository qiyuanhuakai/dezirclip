import { useState, useEffect, useLayoutEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useBackendAppearance } from "../../../shared/hooks/useBackendAppearance";
import { useAuxiliaryWindowReady } from "../../../shared/hooks/useAuxiliaryWindowReady";
import { matchesRegionFrameLayout } from "../../../shared/lib/regionFrameLayout";
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

type RegionCaptureSession = { token: string; x: number; y: number; width: number; height: number; scale: number; generation: number };
type RegionSessionChange = { token: string | null; generation: number };
type RegionBackdrop = { url: string; session: RegionCaptureSession; revision: number };

const matchesFrameLayout = (session: RegionCaptureSession) =>
  matchesRegionFrameLayout({ frameWidth: session.width, frameHeight: session.height, scale: session.scale,
    viewportWidth: window.innerWidth, viewportHeight: window.innerHeight });

const MIN_SELECTION_SIZE = 10;
/**
 * A cancelled selection has no capture to make, and the overlay is fullscreen —
 * while it sits hidden it still holds a whole display's worth of compositor
 * surface. Handing the hide to the backend lets it drop the window's memory
 * target too, which is what gives the memory back. Success parks it only after
 * the capture reply, using its token so a new selection cannot be hidden.
 */
const parkRegionSelectWindow = async (sessionToken?: string) => {
  await invoke("hide_region_select", { sessionToken }).catch(() => undefined);
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
  scale: number | { x: number; y: number }
): CaptureRect => {
  const scaleX = typeof scale === "number" ? scale : scale.x;
  const scaleY = typeof scale === "number" ? scale : scale.y;
  const x = Math.round(origin.x + rect.x * scaleX);
  const y = Math.round(origin.y + rect.y * scaleY);
  return {
    x,
    y,
    width: Math.round(origin.x + (rect.x + rect.width) * scaleX) - x,
    height: Math.round(origin.y + (rect.y + rect.height) * scaleY) - y,
  };
};

const RegionSelectWindow = ({ onSelect, onCancel }: RegionSelectWindowProps) => {
  const [selection, setSelection] = useState<Selection | null>(null);
  const [dragging, setDragging] = useState(false);
  const [boxVisible, setBoxVisible] = useState(false);
  const [backdrop, setBackdrop] = useState<RegionBackdrop | null>(null);
  const [backdropReady, setBackdropReady] = useState(false);
  const [listenersReady, setListenersReady] = useState(false);

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
  const capturePending = useRef<RegionCaptureSession | null>(null);
  const activeSession = useRef<RegionCaptureSession | null>(null);
  const readySession = useRef<RegionCaptureSession | null>(null);
  const pendingSession = useRef<RegionCaptureSession | null>(null);
  const backdropRevision = useRef(0);
  const serverGeneration = useRef(-1);
  const visibilityRevision = useRef(0);
  const announcedSession = useRef<string | null>(null);
  const objectUrl = useRef<string | null>(null);
  const decodingImage = useRef<HTMLImageElement | null>(null);
  const displayedImage = useRef<HTMLImageElement | null>(null);

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

  const clearBackdrop = useCallback(() => {
    backdropRevision.current += 1;
    readySession.current = null;
    pendingSession.current = null;
    activeSession.current = null;
    if (decodingImage.current) decodingImage.current.src = "";
    decodingImage.current = null;
    displayedImage.current?.removeAttribute("src");
    if (objectUrl.current) URL.revokeObjectURL(objectUrl.current);
    objectUrl.current = null;
    setBackdrop(null);
    setBackdropReady(false);
    setDragging(false);
    clearSelection();
  }, [clearSelection]);

  useLayoutEffect(() => {
    if (!backdrop || backdrop.revision !== backdropRevision.current || objectUrl.current !== backdrop.url || !matchesFrameLayout(backdrop.session)) return;
    readySession.current = backdrop.session;
    pendingSession.current = null;
    setBackdropReady(true);
  }, [backdrop]);

  useEffect(() => {
    let disposed = false;
    const cleanups: UnlistenFn[] = [];
    const readNativeVisible = async (revision: number): Promise<boolean | null> => {
      while (!disposed && revision === backdropRevision.current) {
        const request = visibilityRevision.current;
        try {
          const visible = await getCurrentWindow().isVisible();
          if (disposed || revision !== backdropRevision.current) return null;
          if (request === visibilityRevision.current) return visible;
        } catch (error) {
          if (disposed || revision !== backdropRevision.current) return null;
          if (request === visibilityRevision.current) throw error;
        }
      }
      return null;
    };
    const syncBackdrop = async (change?: RegionSessionChange) => {
      if (disposed) return;
      if (change) {
        if (!Number.isSafeInteger(change.generation) || change.generation < serverGeneration.current) return;
        serverGeneration.current = change.generation;
        if (change.token === null) { clearBackdrop(); return; }
        announcedSession.current = change.token;
        if (readySession.current?.token === change.token && readySession.current.generation === change.generation && matchesFrameLayout(readySession.current)) return;
        clearBackdrop();
      }
      const initialRevision = backdropRevision.current;
      let requestRevision = initialRevision;
      try {
        const visible = await readNativeVisible(initialRevision);
        if (disposed || initialRevision !== backdropRevision.current) return;
        if (visible === null) return;
        if (!visible) { clearBackdrop(); return; }
        const session = await invoke<RegionCaptureSession>("get_region_capture_session");
        if (disposed || initialRevision !== backdropRevision.current || session.generation < serverGeneration.current) return;
        serverGeneration.current = session.generation;
        announcedSession.current = session.token;
        if (change && (session.token !== change.token || session.generation !== change.generation)) return;
        if (!matchesFrameLayout(session)) { clearBackdrop(); return; }
        if (readySession.current?.token === session.token && readySession.current.generation === session.generation) return;
        if (pendingSession.current?.token === session.token && pendingSession.current.generation === session.generation) return;
        clearBackdrop();
        const revision = backdropRevision.current;
        requestRevision = revision;
        pendingSession.current = session;
        const bytes = await invoke<ArrayBuffer | number[]>("get_region_capture_frame", { sessionToken: session.token });
        if (disposed || revision !== backdropRevision.current || session.generation !== serverGeneration.current) return;
        const data = bytes instanceof ArrayBuffer ? bytes : new Uint8Array(bytes).buffer;
        const url = URL.createObjectURL(new Blob([data], { type: "image/png" }));
        objectUrl.current = url;
        const image = new Image();
        decodingImage.current = image;
        image.src = url;
        try { await image.decode(); }
        catch (error) {
          URL.revokeObjectURL(url);
          if (disposed || revision !== backdropRevision.current) return;
          throw error;
        }
        if (disposed || revision !== backdropRevision.current || objectUrl.current !== url || session.generation !== serverGeneration.current) {
          URL.revokeObjectURL(url);
          return;
        }
        if (image.naturalWidth !== session.width || image.naturalHeight !== session.height || !matchesFrameLayout(session)) {
          clearBackdrop();
          return;
        }
        const stillVisible = await readNativeVisible(revision);
        if (disposed || revision !== backdropRevision.current || objectUrl.current !== url || session.generation !== serverGeneration.current) {
          URL.revokeObjectURL(url);
          return;
        }
        if (stillVisible === null) return;
        if (!stillVisible) { clearBackdrop(); return; }
        decodingImage.current = null;
        setBackdrop({ url, session, revision });
      } catch (error) {
        if (!disposed && requestRevision === backdropRevision.current) {
          clearBackdrop();
          console.warn("Region capture preview unavailable", error);
        }
      }
    };
    const resize = () => { visibilityRevision.current += 1; clearBackdrop(); if (!document.hidden) void syncBackdrop(); };
    const visibility = () => { visibilityRevision.current += 1; if (document.hidden) clearBackdrop(); else void syncBackdrop(); };
    const focus = () => { visibilityRevision.current += 1; if (!document.hidden) void syncBackdrop(); };
    const hide = () => clearBackdrop();
    const nativeFocus = (focused: boolean) => {
      if (focused) { focus(); return; }
      const revision = backdropRevision.current;
      const request = ++visibilityRevision.current;
      void getCurrentWindow().isVisible().then(visible => {
        if (!disposed && revision === backdropRevision.current && request === visibilityRevision.current && !visible) clearBackdrop();
      }).catch(() => { if (!disposed && revision === backdropRevision.current && request === visibilityRevision.current) clearBackdrop(); });
    };
    window.addEventListener("resize", resize);
    window.addEventListener("focus", focus);
    window.addEventListener("pagehide", hide);
    document.addEventListener("visibilitychange", visibility);
    void (async () => {
      const registrations = [
        () => listen<RegionSessionChange>("region-capture-session-changed", event => { void syncBackdrop(event.payload); }),
        () => getCurrentWindow().onFocusChanged(event => nativeFocus(event.payload)),
        () => getCurrentWindow().onScaleChanged(resize),
      ];
      try {
        for (const register of registrations) {
          const unlisten = await register();
          if (disposed) { unlisten(); return; }
          cleanups.push(unlisten);
        }
        setListenersReady(true);
        void syncBackdrop();
      } catch (error) { if (!disposed) console.warn("Region capture preview listeners unavailable", error); }
    })();
    return () => {
      disposed = true;
      for (const cleanup of cleanups) cleanup();
      window.removeEventListener("resize", resize);
      window.removeEventListener("focus", focus);
      window.removeEventListener("pagehide", hide);
      document.removeEventListener("visibilitychange", visibility);
      clearBackdrop();
    };
  }, [clearBackdrop]);

  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      if (!readySession.current || capturePending.current?.generation === readySession.current.generation || !matchesFrameLayout(readySession.current)) return;
      activeSession.current = readySession.current;
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
    if (!dragging || (capturePending.current && capturePending.current.generation === activeSession.current?.generation)) return;
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

    let sessionToken: string | undefined;
    let captureGeneration: number | undefined;
    try {
      const session = activeSession.current;
      if (!session || readySession.current !== session || !matchesFrameLayout(session)) throw new Error("RegionCaptureSessionExpired");
      capturePending.current = session;
      captureGeneration = session.generation;
      sessionToken = session.token;
      const imageBox = displayedImage.current?.getBoundingClientRect();
      if (!imageBox || imageBox.width <= 0 || imageBox.height <= 0) throw new Error("RegionCaptureLayoutUnavailable");
      const captureRect = toPhysicalCaptureRect({ ...rect, x: rect.x - imageBox.left, y: rect.y - imageBox.top }, session, {
        x: session.width / imageBox.width,
        y: session.height / imageBox.height,
      });
      clearSelection();
      clearBackdrop();
      await invoke("capture_region", { ...captureRect, sessionToken: session.token });
      await parkRegionSelectWindow(session.token);
      if (announcedSession.current === sessionToken) onSelect?.(captureRect);
      return;
    } catch (err) {
      console.error("Failed to capture selected region", err);
    } finally {
      if (capturePending.current?.token === sessionToken) capturePending.current = null;
    }

    if (captureGeneration === serverGeneration.current) clearSelection();
    if (sessionToken) await parkRegionSelectWindow(sessionToken);
  }, [dragging, onSelect, cancelFrame, clearSelection, clearBackdrop]);

  // ESC key handler
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (capturePending.current?.generation === serverGeneration.current) return;
      if (e.key === "Escape") {
        clearBackdrop();
        setDragging(false);
        onCancel?.();
        parkRegionSelectWindow().catch(() => undefined);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onCancel, clearBackdrop]);

  useAuxiliaryWindowReady(listenersReady);

  return (
    <div
      className={`region-select-window${backdropReady ? " is-ready" : ""}`}
      data-testid="region-select-overlay"
      data-ready={backdropReady}
      onMouseDown={handleMouseDown}
      onMouseMove={handleMouseMove}
      onMouseUp={handleMouseUp}
    >
      {backdrop && <img ref={displayedImage} className="region-select-window__backdrop" src={backdrop.url} alt="" draggable={false} />}
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
