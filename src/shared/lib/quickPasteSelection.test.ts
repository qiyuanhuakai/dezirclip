import { describe, expect, it } from "vitest";
import { applyHistoryUpdate } from "./historyInsert";
import { resolveActiveIndex, selectionAfterUpdate } from "./quickPasteSelection";
import type { ClipboardEntry } from "../types";

const entry = (id: number, timestamp: number, isPinned = false): ClipboardEntry =>
    ({
        id,
        content: `entry ${id}`,
        preview: `entry ${id}`,
        content_type: "text",
        timestamp,
        is_pinned: isPinned
    }) as ClipboardEntry;

describe("resolveActiveIndex", () => {
    it("reports no position when there is nothing to select", () => {
        expect(resolveActiveIndex([], 7)).toBe(-1);
    });

    it("falls back to the top when nothing is selected yet", () => {
        expect(resolveActiveIndex([entry(1, 100), entry(2, 90)], null)).toBe(0);
    });

    // The reported failure: the list is [A, B, C] with B selected, a re-copy of C
    // moves it to the front, and the stored index 1 now points at A. Keeping the
    // selection as an id is what stops that.
    it("follows the selected entry when an update reorders the list", () => {
        const before = [entry(1, 100), entry(2, 90), entry(3, 80)];
        expect(resolveActiveIndex(before, 2)).toBe(1);

        const { entries } = applyHistoryUpdate(before, entry(3, 110));
        expect(entries.map((e) => e.id)).toEqual([3, 1, 2]);

        // What the stored index used to resolve to once the list moved under it.
        expect(entries[1].id).toBe(1);
        // Where the entry that was actually selected ended up.
        expect(resolveActiveIndex(entries, 2)).toBe(2);
    });

    it("falls back to the top when the selected entry is gone", () => {
        expect(resolveActiveIndex([entry(1, 100), entry(2, 90)], 99)).toBe(0);
    });

    it("falls back to the top when a search filters the selected entry out", () => {
        expect(resolveActiveIndex([entry(1, 100)], 2)).toBe(0);
    });
});

describe("selectionAfterUpdate", () => {
    it("follows a new entry even when a pinned entry sorts above it", () => {
        const incoming = entry(2, 110);
        const chosen = selectionAfterUpdate(null, false, incoming);
        const { entries } = applyHistoryUpdate([entry(1, 100, true)], incoming);

        expect(entries.map((e) => e.id)).toEqual([1, 2]);
        expect(chosen).toBe(2);
        // The point of the case: the entry is at position 1, not 0.
        expect(resolveActiveIndex(entries, chosen)).toBe(1);
    });

    it("keeps the selection on the same entry when one already listed is updated", () => {
        const incoming = entry(3, 110);
        const chosen = selectionAfterUpdate(2, true, incoming);
        const { entries } = applyHistoryUpdate(
            [entry(1, 100), entry(2, 90), entry(3, 80)],
            incoming
        );

        expect(chosen).toBe(2);
        expect(entries.map((e) => e.id)).toEqual([3, 1, 2]);
        expect(resolveActiveIndex(entries, chosen)).toBe(2);
    });

    it("adopts the new entry when nothing was selected", () => {
        expect(selectionAfterUpdate(null, false, entry(4, 10))).toBe(4);
    });
});
