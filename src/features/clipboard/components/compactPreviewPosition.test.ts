import { describe, expect, it } from "vitest";
import { pickPreviewPosition } from "./compactPreviewPosition";

describe("compact preview positioning", () => {
  it("aligns side placement with the hovered item when preview must flip upward", () => {
    const target = pickPreviewPosition({
      anchorX: 760,
      anchorY: 560,
      anchorRect: { left: 16, top: 520, right: 780, bottom: 600 },
      widthPx: 240,
      heightPx: 180,
      monitorPos: { x: 0, y: 0 },
      monitorSize: { width: 1280, height: 720 },
      margin: 10,
      offset: 12,
      avoidRect: { left: 0, top: 0, right: 800, bottom: 640 },
    });

    expect(target.x).toBe(812);
    expect(target.y).toBe(420);
  });

  it("keeps upward placement near the hovered item when side placement cannot avoid the main window", () => {
    const target = pickPreviewPosition({
      anchorX: 620,
      anchorY: 560,
      anchorRect: { left: 24, top: 520, right: 720, bottom: 600 },
      widthPx: 360,
      heightPx: 180,
      monitorPos: { x: 0, y: 0 },
      monitorSize: { width: 800, height: 640 },
      margin: 10,
      offset: 12,
      avoidRect: { left: 0, top: 0, right: 800, bottom: 640 },
    });

    expect(target.y).toBe(420);
  });
});
