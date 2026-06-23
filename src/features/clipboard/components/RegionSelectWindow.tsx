import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { applyThemeClass, applyModeClass } from "../../../shared/lib/themeRuntime";
import "./RegionSelectWindow.css";

type Selection = {
  startX: number;
  startY: number;
  endX: number;
  endY: number;
};

type RegionSelectWindowProps = {
  onSelect?: (result: { x: number; y: number; width: number; height: number }) => void;
  onCancel?: () => void;
};

const MIN_SELECTION_SIZE = 10;

const hideRegionSelectWindow = async () => {
  await getCurrentWindow().setFocusable(false);
  await getCurrentWindow().hide();
};

const normalizeRect = (sel: Selection) => {
  const x = Math.min(sel.startX, sel.endX);
  const y = Math.min(sel.startY, sel.endY);
  const width = Math.abs(sel.endX - sel.startX);
  const height = Math.abs(sel.endY - sel.startY);
  return { x, y, width, height };
};

const RegionSelectWindow = ({ onSelect, onCancel }: RegionSelectWindowProps) => {
  const [selection, setSelection] = useState<Selection | null>(null);
  const [dragging, setDragging] = useState(false);

  // Apply theme from localStorage
  useEffect(() => {
    const theme = localStorage.getItem("tiez_theme") || "mica";
    const colorMode = localStorage.getItem("tiez_color_mode") || "light";
    applyThemeClass(document.documentElement, document.body, theme);
    applyModeClass(
      document.documentElement,
      document.body,
      colorMode === "dark" ? "dark" : "light"
    );
    document.body.classList.add("region-select");
  }, []);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setSelection({ startX: e.clientX, startY: e.clientY, endX: e.clientX, endY: e.clientY });
    setDragging(true);
  }, []);

  const handleMouseMove = useCallback(
    (e: React.MouseEvent) => {
      if (!dragging) return;
      setSelection((prev) =>
        prev ? { ...prev, endX: e.clientX, endY: e.clientY } : prev
      );
    },
    [dragging]
  );

  const handleMouseUp = useCallback(async () => {
    if (!dragging || !selection) return;
    setDragging(false);

    const rect = normalizeRect(selection);
    if (rect.width < MIN_SELECTION_SIZE || rect.height < MIN_SELECTION_SIZE) {
      setSelection(null);
      return;
    }

    try {
      await invoke("capture_region", {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
      });
      onSelect?.({ x: rect.x, y: rect.y, width: rect.width, height: rect.height });
    } catch {
      // capture_region not available yet (Task 37 pending) — still emit selection
      onSelect?.({ x: rect.x, y: rect.y, width: rect.width, height: rect.height });
    }

    setSelection(null);
    await hideRegionSelectWindow().catch(() => undefined);
  }, [dragging, selection, onSelect]);

  // ESC key handler
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setSelection(null);
        setDragging(false);
        onCancel?.();
        hideRegionSelectWindow().catch(() => undefined);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onCancel]);

  // Cleanup on unmount — no stale state errors
  useEffect(() => {
    return () => {
      setSelection(null);
      setDragging(false);
    };
  }, []);

  const rect = selection ? normalizeRect(selection) : null;
  const hasValidSize =
    rect !== null && rect.width >= MIN_SELECTION_SIZE && rect.height >= MIN_SELECTION_SIZE;

  return (
    <div
      className="region-select-window"
      data-testid="region-select-overlay"
      onMouseDown={handleMouseDown}
      onMouseMove={handleMouseMove}
      onMouseUp={handleMouseUp}
    >
      {selection && hasValidSize && (
        <div
          className="region-select-window__selection"
          data-testid="region-select-box"
          style={{
            left: rect.x,
            top: rect.y,
            width: rect.width,
            height: rect.height,
          }}
        >
          <div className="region-select-window__dimensions" data-testid="region-select-dimensions">
            {rect.width} × {rect.height}
          </div>
        </div>
      )}
    </div>
  );
};

export default RegionSelectWindow;
