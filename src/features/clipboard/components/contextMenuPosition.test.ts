import { describe, expect, it } from "vitest";
import { resolveContextMenuPoint } from "./contextMenuPosition";

const anchorRect = { left: 40, top: 120, right: 420, bottom: 180 };
const viewport = { width: 800, height: 600, scale: 2 };

describe("context menu positioning", () => {
  it("keeps normal CSS viewport coordinates at the pointer", () => {
    expect(resolveContextMenuPoint({ clientX: 240, clientY: 160, anchorRect, viewport })).toEqual({
      x: 240,
      y: 160,
    });
  });

  it("normalizes physical coordinates reported by Linux WebKit into CSS pixels", () => {
    expect(resolveContextMenuPoint({ clientX: 480, clientY: 320, anchorRect, viewport })).toEqual({
      x: 480,
      y: 320,
    });
    expect(resolveContextMenuPoint({ clientX: 1200, clientY: 700, anchorRect, viewport })).toEqual({
      x: 600,
      y: 350,
    });
  });

  it("falls back to the card position when pointer coordinates are unusable", () => {
    expect(resolveContextMenuPoint({ clientX: 5000, clientY: 3000, anchorRect, viewport })).toEqual({
      x: 40,
      y: 120,
    });
  });
});
