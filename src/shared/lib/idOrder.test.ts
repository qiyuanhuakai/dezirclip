import { describe, it, expect } from "vitest";
import { sameIdOrder } from "./idOrder";

describe("sameIdOrder", () => {
  it("treats a rebuilt but unchanged ordering as unchanged", () => {
    expect(sameIdOrder([3, 1, 2], [3, 1, 2])).toBe(true);
    expect(sameIdOrder([], [])).toBe(true);
  });

  it("sees a reordering as a change", () => {
    expect(sameIdOrder([1, 2, 3], [2, 1, 3])).toBe(false);
  });

  it("sees a different length as a change even when the prefix matches", () => {
    expect(sameIdOrder([1, 2], [1, 2, 3])).toBe(false);
    expect(sameIdOrder([1, 2, 3], [1, 2])).toBe(false);
  });

  it("sees a row appearing or disappearing as a change", () => {
    expect(sameIdOrder([1, 9, 2], [1, 2])).toBe(false);
  });
});
