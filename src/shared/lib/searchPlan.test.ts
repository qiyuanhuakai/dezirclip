import { describe, it, expect } from "vitest";
import {
  compareForList,
  compareForScore,
  planNeedsFuzzyIndex,
  planSearch
} from "./searchPlan";

describe("planSearch", () => {
  it("defers to the server once the debounced snapshot has landed", () => {
    expect(planSearch({ search: "abc", debouncedSearch: "abc" })).toEqual({
      kind: "server"
    });
  });

  it("still filters locally while the debounce is still catching up", () => {
    expect(planSearch({ search: "abc", debouncedSearch: "ab" })).toEqual({
      kind: "fuzzy",
      terms: ["abc"]
    });
  });

  it("treats an empty box as a plain list", () => {
    expect(planSearch({ search: "", debouncedSearch: "" })).toEqual({ kind: "plain" });
  });

  it("keeps a whitespace-only box unsorted, matching the old terms.length === 0 branch", () => {
    expect(planSearch({ search: "   ", debouncedSearch: "" })).toEqual({
      kind: "unordered"
    });
  });

  it("routes a tag: prefix to the tag matcher with the prefix stripped", () => {
    expect(planSearch({ search: "TAG:Work", debouncedSearch: "" })).toEqual({
      kind: "tag",
      term: "work"
    });
  });

  it("splits a content query into its terms", () => {
    expect(planSearch({ search: "  alpha   beta ", debouncedSearch: "" })).toEqual({
      kind: "fuzzy",
      terms: ["alpha", "beta"]
    });
  });
});

describe("planNeedsFuzzyIndex", () => {
  // Only the fuzzy branch reads the index back, so every other plan has to
  // answer false: that is what keeps a kilobyte of heap per history entry
  // from being held while the user is just browsing and scrolling.
  it("is false for every branch except fuzzy", () => {
    expect(planNeedsFuzzyIndex({ kind: "server" })).toBe(false);
    expect(planNeedsFuzzyIndex({ kind: "plain" })).toBe(false);
    expect(planNeedsFuzzyIndex({ kind: "unordered" })).toBe(false);
    expect(planNeedsFuzzyIndex({ kind: "tag", term: "work" })).toBe(false);
    expect(planNeedsFuzzyIndex({ kind: "fuzzy", terms: ["a"] })).toBe(true);
  });
});

describe("compareForList", () => {
  const at = (timestamp: number, is_pinned = false, pinned_order = 0) => ({
    is_pinned,
    pinned_order,
    timestamp
  });

  it("floats pinned entries above unpinned ones regardless of time", () => {
    expect([at(1), at(999, true)].sort(compareForList)).toEqual([at(999, true), at(1)]);
  });

  it("orders pinned entries by pinned_order then timestamp", () => {
    const rows = [at(10, true, 1), at(20, true, 5), at(30, true, 5)];
    expect(rows.sort(compareForList)).toEqual([
      at(30, true, 5),
      at(20, true, 5),
      at(10, true, 1)
    ]);
  });

  it("orders unpinned entries newest first", () => {
    expect([at(1), at(3), at(2)].sort(compareForList)).toEqual([at(3), at(2), at(1)]);
  });
});

describe("compareForScore", () => {
  const row = (timestamp: number, score: number, is_pinned = false, pinned_order = 0) => ({
    item: { is_pinned, pinned_order, timestamp },
    score
  });

  it("ranks unpinned entries by score before the timestamp", () => {
    const rows = [row(999, 0.1), row(1, 0.9)];
    expect(rows.sort(compareForScore)).toEqual([row(1, 0.9), row(999, 0.1)]);
  });

  it("breaks a score tie on the timestamp", () => {
    expect([row(1, 0.5), row(9, 0.5)].sort(compareForScore)).toEqual([row(9, 0.5), row(1, 0.5)]);
  });

  it("keeps pinned entries above a better-scoring unpinned one", () => {
    const rows = [row(1, 0.99), row(500, 0.01, true, 2)];
    expect(rows.sort(compareForScore)).toEqual([row(500, 0.01, true, 2), row(1, 0.99)]);
  });

  it("ignores score between two pinned entries and orders by pinned_order", () => {
    const rows = [row(1, 0.99, true, 1), row(900, 0.01, true, 4)];
    expect(rows.sort(compareForScore)).toEqual([row(900, 0.01, true, 4), row(1, 0.99, true, 1)]);
  });
});
