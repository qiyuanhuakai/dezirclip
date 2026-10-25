import { describe, expect, it } from "vitest";
import { matchesRegionFrameLayout, type RegionFrameLayout } from "./regionFrameLayout";

const highScale: RegionFrameLayout = {
  frameWidth: 2560,
  frameHeight: 1440,
  scale: 3.5,
  viewportWidth: 731,
  viewportHeight: 411,
};

describe("matchesRegionFrameLayout", () => {
  it("accepts floor and ceil CSS viewports for an independent 2560×1440 frame at 350%", () => {
    for (const viewportWidth of [731, 732]) {
      for (const viewportHeight of [411, 412]) {
        expect(matchesRegionFrameLayout({ ...highScale, viewportWidth, viewportHeight })).toBe(true);
      }
    }
  });

  it("accepts the exact 3840×2160 frame and 1920×1080 viewport at 200%", () => {
    expect(matchesRegionFrameLayout({ frameWidth: 3840, frameHeight: 2160, scale: 2, viewportWidth: 1920, viewportHeight: 1080 })).toBe(true);
  });

  it("limits quantization to less than one CSS pixel instead of one physical pixel", () => {
    expect(matchesRegionFrameLayout({ frameWidth: 1366, frameHeight: 768, scale: 1.5, viewportWidth: 910, viewportHeight: 512 })).toBe(true);
    expect(matchesRegionFrameLayout({ frameWidth: 1366, frameHeight: 768, scale: 1.5, viewportWidth: 911, viewportHeight: 512 })).toBe(true);
    expect(matchesRegionFrameLayout({ frameWidth: 3840, frameHeight: 2160, scale: 2, viewportWidth: 1921, viewportHeight: 1080 })).toBe(false);
    expect(matchesRegionFrameLayout({ frameWidth: 3840, frameHeight: 2160, scale: 2, viewportWidth: 1920, viewportHeight: 1079 })).toBe(false);
  });

  it("rejects a resized viewport or a stale scale on either axis", () => {
    for (const change of [{ viewportWidth: 900, viewportHeight: 600 }, { viewportWidth: 900 }, { viewportHeight: 600 }, { scale: 4 }]) {
      expect(matchesRegionFrameLayout({ ...highScale, ...change })).toBe(false);
    }
  });

  it("rejects missing pixel extents and non-finite or non-positive geometry", () => {
    for (const field of ["frameWidth", "frameHeight", "scale", "viewportWidth", "viewportHeight"] as const) {
      for (const value of [0, -1, Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY]) {
        expect(matchesRegionFrameLayout({ ...highScale, [field]: value })).toBe(false);
      }
    }
    expect(matchesRegionFrameLayout({ ...highScale, frameWidth: 2560.5 })).toBe(false);
    expect(matchesRegionFrameLayout({ ...highScale, frameHeight: Number.MAX_VALUE })).toBe(false);
  });
});
