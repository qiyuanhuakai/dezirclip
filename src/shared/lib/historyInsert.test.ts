import { describe, expect, it } from "vitest";
import { applyHistoryUpdate, insertHistoryItem } from "./historyInsert";
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

describe("insertHistoryItem", () => {
    it("puts a new entry at the top of the unpinned block", () => {
        const list = [entry(1, 100), entry(2, 90)];
        const result = insertHistoryItem(list, entry(3, 110));
        expect(result.map((e) => e.id)).toEqual([3, 1, 2]);
    });

    it("keeps pinned entries above unpinned ones", () => {
        const list = [entry(1, 100, true)];
        const result = insertHistoryItem(list, entry(2, 50));
        expect(result.map((e) => e.id)).toEqual([1, 2]);
    });

    it("puts a pinned entry above every unpinned entry", () => {
        const list = [entry(1, 10), entry(2, 20)];
        const result = insertHistoryItem(list, entry(3, 5, true));
        expect(result.map((e) => e.id)).toEqual([3, 1, 2]);
    });

    it("orders pinned entries among themselves by timestamp", () => {
        const list = [entry(1, 300, true), entry(2, 200, true)];
        const result = insertHistoryItem(list, entry(3, 250, true));
        expect(result.map((e) => e.id)).toEqual([1, 3, 2]);
    });

    it("does not mutate the list it was given", () => {
        const list = [entry(1, 100)];
        insertHistoryItem(list, entry(2, 200));
        expect(list.map((e) => e.id)).toEqual([1]);
    });
});

describe("applyHistoryUpdate", () => {
    it("reports a genuinely new entry as absent", () => {
        const result = applyHistoryUpdate([entry(1, 100)], entry(2, 200));
        expect(result.wasPresent).toBe(false);
        expect(result.entries.map((e) => e.id)).toEqual([2, 1]);
    });

    it("moves an entry that is already listed instead of duplicating it", () => {
        const list = [entry(1, 100), entry(2, 90)];
        const result = applyHistoryUpdate(list, entry(2, 300));
        expect(result.wasPresent).toBe(true);
        expect(result.entries.map((e) => e.id)).toEqual([2, 1]);
    });

    it("re-sorts an entry that was already listed behind a pinned one", () => {
        const list = [entry(1, 100, true), entry(2, 90)];
        const result = applyHistoryUpdate(list, entry(2, 300));
        expect(result.entries.map((e) => e.id)).toEqual([1, 2]);
    });
});
