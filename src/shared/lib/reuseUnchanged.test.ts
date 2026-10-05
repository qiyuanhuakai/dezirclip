import { describe, it, expect } from "vitest";
import { reuseUnchangedEntries } from "./reuseUnchanged";
import type { ClipboardEntry } from "../types";

const makeEntry = (over: Partial<ClipboardEntry> = {}): ClipboardEntry => ({
    id: 1,
    content_type: "text",
    content: "hello",
    source_app: "app",
    timestamp: 1700000000,
    preview: "hello",
    is_pinned: false,
    tags: [],
    ...over
});

describe("reuseUnchangedEntries", () => {
    it("hands back the previous object when nothing changed", () => {
        const old = makeEntry({ id: 5 });
        const out = reuseUnchangedEntries([old], [makeEntry({ id: 5 })]);
        expect(out[0]).toBe(old);
    });

    it("hands back a fresh object when a field changed", () => {
        const old = makeEntry({ id: 5, content: "before" });
        const next = makeEntry({ id: 5, content: "after" });
        expect(reuseUnchangedEntries([old], [next])[0]).toBe(next);
    });

    it("treats a freshly built tags array with the same members as unchanged", () => {
        const old = makeEntry({ id: 5, tags: ["a", "b"] });
        const out = reuseUnchangedEntries([old], [makeEntry({ id: 5, tags: ["a", "b"] })]);
        expect(out[0]).toBe(old);
    });

    it("notices a reordered or resized tags array", () => {
        const old = makeEntry({ id: 5, tags: ["a", "b"] });
        const swapped = makeEntry({ id: 5, tags: ["b", "a"] });
        const longer = makeEntry({ id: 5, tags: ["a", "b", "c"] });
        expect(reuseUnchangedEntries([old], [swapped])[0]).toBe(swapped);
        expect(reuseUnchangedEntries([old], [longer])[0]).toBe(longer);
    });

    it("passes id 0 session entries through untouched", () => {
        const a = makeEntry({ id: 0, content: "one" });
        const b = makeEntry({ id: 0, content: "one" });
        expect(reuseUnchangedEntries([a], [b])[0]).toBe(b);
    });

    it("returns the new list untouched when there is nothing to compare against", () => {
        const next = [makeEntry({ id: 5 })];
        expect(reuseUnchangedEntries([], next)).toBe(next);
        expect(reuseUnchangedEntries([makeEntry({ id: 0 })], next)).toBe(next);
    });

    it("keeps the new order and the new length", () => {
        const prev = [makeEntry({ id: 1 }), makeEntry({ id: 2 }), makeEntry({ id: 3 })];
        const next = [makeEntry({ id: 3 }), makeEntry({ id: 2 })];
        const out = reuseUnchangedEntries(prev, next);
        expect(out.map((e) => e.id)).toEqual([3, 2]);
        expect(out[0]).toBe(prev[2]);
        expect(out[1]).toBe(prev[1]);
    });

    it("does not mistake two entries for one when the shape differs", () => {
        const old = makeEntry({ id: 5 });
        const extra = { ...makeEntry({ id: 5 }), questionCount: 2 };
        expect(reuseUnchangedEntries([old], [extra])[0]).toBe(extra);
    });
});
