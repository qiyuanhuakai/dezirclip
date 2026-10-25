import { describe, expect, it } from "vitest";
import { previewPhysicalPosition } from "./previewPhysicalPosition";

describe("preview physical position", () => {
    it("accepts the fractional DOM placement observed on a 150% monitor", () => {
        expect(previewPhysicalPosition({ x: 1157, y: 271.00000762939453 })).toEqual({ x: 1157, y: 271 });
    });

    it("rounds both axes on monitors left of and above the primary display", () => {
        expect(previewPhysicalPosition({ x: -1200.75, y: -200.25 })).toEqual({ x: -1201, y: -200 });
    });

    it("preserves integer coordinates at either signed boundary", () => {
        expect(previewPhysicalPosition({ x: -2_147_483_648, y: 2_147_483_647 }))
            .toEqual({ x: -2_147_483_648, y: 2_147_483_647 });
    });

    it("rejects invalid or out-of-range placement instead of invoking native movement", () => {
        for (const coordinate of [NaN, Infinity, -Infinity, 2_147_483_648, -2_147_483_649]) {
            expect(() => previewPhysicalPosition({ x: coordinate, y: 0 })).toThrow("PreviewPositionUnavailable");
            expect(() => previewPhysicalPosition({ x: 0, y: coordinate })).toThrow("PreviewPositionUnavailable");
        }
    });
});
