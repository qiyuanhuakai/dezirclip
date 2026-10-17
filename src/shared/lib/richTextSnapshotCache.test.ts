import { describe, expect, it } from "vitest";
import {
    DEFAULT_RICH_TEXT_SNAPSHOT_CACHE,
    MAX_RICH_TEXT_SNAPSHOT_CACHE,
    MIN_RICH_TEXT_SNAPSHOT_CACHE,
    RICH_TEXT_SNAPSHOT_CACHE_STEP,
    resolveRichTextSnapshotCacheLimit,
    snapshotEvictionCount,
} from "./richTextSnapshotCache";

describe("resolveRichTextSnapshotCacheLimit", () => {
    it("keeps a value the user actually chose", () => {
        expect(resolveRichTextSnapshotCacheLimit("0")).toBe(0);
        expect(resolveRichTextSnapshotCacheLimit("60")).toBe(60);
        expect(resolveRichTextSnapshotCacheLimit("500")).toBe(500);
    });

    it("clamps to the supported range rather than rejecting the setting", () => {
        expect(resolveRichTextSnapshotCacheLimit("-5")).toBe(MIN_RICH_TEXT_SNAPSHOT_CACHE);
        expect(resolveRichTextSnapshotCacheLimit("99999")).toBe(MAX_RICH_TEXT_SNAPSHOT_CACHE);
    });

    // A missing key, a blank value, or a hand-edited non-number must not read
    // as 0: that would silently mean "caching disabled" and cost the user the
    // memory setting they believed they still had.
    it("falls back to the default for anything unreadable", () => {
        for (const raw of [undefined, null, "", "   ", "abc", "12abc", "NaN"]) {
            expect(resolveRichTextSnapshotCacheLimit(raw)).toBe(
                DEFAULT_RICH_TEXT_SNAPSHOT_CACHE,
            );
        }
    });

    it("falls back rather than going negative on a negative number", () => {
        // "-5" is numeric, so it clamps to 0 -- but "not-a-number" must not,
        // because 0 is a real setting with a real meaning.
        expect(resolveRichTextSnapshotCacheLimit("-5")).toBe(0);
        expect(resolveRichTextSnapshotCacheLimit("nope")).toBe(
            DEFAULT_RICH_TEXT_SNAPSHOT_CACHE,
        );
    });

    it("keeps the pre-existing default reachable so upgrading changes nothing", () => {
        expect(resolveRichTextSnapshotCacheLimit(String(DEFAULT_RICH_TEXT_SNAPSHOT_CACHE))).toBe(
            DEFAULT_RICH_TEXT_SNAPSHOT_CACHE,
        );
    });

    it("offers a step that divides the range", () => {
        expect(MAX_RICH_TEXT_SNAPSHOT_CACHE % RICH_TEXT_SNAPSHOT_CACHE_STEP).toBe(0);
    });
});

describe("snapshotEvictionCount", () => {
    it("drops nothing while the cache is under the limit", () => {
        expect(snapshotEvictionCount(0, 120)).toBe(0);
        expect(snapshotEvictionCount(119, 120)).toBe(0);
        expect(snapshotEvictionCount(120, 120)).toBe(0);
    });

    it("drops only the overflow", () => {
        expect(snapshotEvictionCount(121, 120)).toBe(1);
        expect(snapshotEvictionCount(200, 120)).toBe(80);
    });

    // Lowering the setting has to shrink an already-full cache immediately,
    // otherwise the memory the user just reclaimed stays held.
    it("empties the cache when the limit drops to zero", () => {
        expect(snapshotEvictionCount(37, 0)).toBe(37);
    });

    it("never reports a negative count", () => {
        expect(snapshotEvictionCount(5, 0)).toBe(5);
        expect(snapshotEvictionCount(5, -1)).toBe(5);
    });
});
