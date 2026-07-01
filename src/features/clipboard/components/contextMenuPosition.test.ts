import { describe, expect, it } from "vitest";
import { resolveContextMenuPoint } from "./contextMenuPosition";

const anchorRect = { left: 40, top: 120, right: 420, bottom: 180 };
const viewport = { width: 800, height: 600, scale: 1 };

describe("context menu positioning", () => {
  it("keeps normal CSS viewport coordinates at the pointer", () => {
    expect(resolveContextMenuPoint({ clientX: 240, clientY: 160, anchorRect, viewport })).toEqual({
      x: 240,
      y: 160,
    });
  });

  it("falls back to the card position when pointer coordinates are unusable", () => {
    expect(resolveContextMenuPoint({ clientX: 5000, clientY: 3000, anchorRect, viewport })).toEqual({
      x: 40,
      y: 120,
    });
  });
});
