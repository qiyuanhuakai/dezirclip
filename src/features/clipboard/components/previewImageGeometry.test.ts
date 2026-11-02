import { describe, expect, it } from "vitest";
import { boundedPreviewZoom, fitPreviewZoom, fittedAxisPan, zoomPreviewScroll } from "./previewImageGeometry";

describe("preview image geometry", () => {
    it("fits wide and tall images without growing a small image", () => {
        expect(fitPreviewZoom({ width: 400, height: 300 }, { width: 2000, height: 1000 })).toBe(0.2);
        expect(fitPreviewZoom({ width: 400, height: 300 }, { width: 100, height: 100 })).toBe(1);
        expect(fitPreviewZoom({ width: 400, height: 300 }, { width: 1000, height: 6000 })).toBe(0.05);
    });
    it("pans a fitted axis and keeps a reachable part of the image inside", () => {
        expect(fittedAxisPan(300, 150, 32)).toBe(32);
        expect(fittedAxisPan(300, 300, -1000)).toBe(-276);
        expect(fittedAxisPan(300, 150, 1000)).toBe(201);
        expect(fittedAxisPan(300, 350, 32)).toBe(0);
        expect(zoomPreviewScroll({ width: 300, height: 300 }, { width: 300, height: 300 },
            { x: 0, y: 0 }, { x: 150, y: 150 }, 1, 2, { x: 50, y: -40 })).toEqual({ x: 50, y: 230 });
    });
    it("bounds explicit zoom and rejects nonfinite factors", () => {
        expect(boundedPreviewZoom(Infinity)).toBe(1);
        expect(boundedPreviewZoom(NaN)).toBe(1);
        expect(boundedPreviewZoom(0.001)).toBe(0.1);
        expect(boundedPreviewZoom(50)).toBe(8);
    });
    it("keeps the image point under the wheel in place", () => {
        expect(zoomPreviewScroll({ width: 400, height: 300 }, { width: 1000, height: 1000 },
            { x: 100, y: 200 }, { x: 150, y: 80 }, 1, 2)).toEqual({ x: 350, y: 480 });
    });
    it("accounts for a fitted image centered within its viewport", () => {
        expect(zoomPreviewScroll({ width: 400, height: 300 }, { width: 1000, height: 500 },
            { x: 0, y: 0 }, { x: 200, y: 150 }, 0.4, 1)).toEqual({ x: 300, y: 100 });
    });
    it("clamps edges and does not invent scroll for invalid zoom", () => {
        expect(zoomPreviewScroll({ width: 400, height: 300 }, { width: 1000, height: 1000 },
            { x: 900, y: 900 }, { x: 400, y: 300 }, 1, 2)).toEqual({ x: 1600, y: 1700 });
        expect(zoomPreviewScroll({ width: 400, height: 300 }, { width: 1000, height: 1000 },
            { x: 10, y: 20 }, { x: 100, y: 100 }, 0, 2)).toEqual({ x: 10, y: 20 });
    });
});
