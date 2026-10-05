import { describe, it, expect } from "vitest";
import { arrowSelectionStep } from "./arrowSelection";

describe("arrowSelectionStep", () => {
  it("advances one row on ArrowDown", () => {
    expect(arrowSelectionStep("ArrowDown", 0, 10)).toBe(1);
    expect(arrowSelectionStep("ArrowDown", 4, 10)).toBe(5);
  });

  it("goes back one row on ArrowUp", () => {
    expect(arrowSelectionStep("ArrowUp", 4, 10)).toBe(3);
    expect(arrowSelectionStep("ArrowUp", 1, 10)).toBe(0);
  });

  it("stops at the last row instead of running past it", () => {
    expect(arrowSelectionStep("ArrowDown", 9, 10)).toBe(9);
    expect(arrowSelectionStep("ArrowDown", 40, 10)).toBe(9);
  });

  it("stops at the first row instead of running below zero", () => {
    expect(arrowSelectionStep("ArrowUp", 0, 10)).toBe(0);
  });

  it("keeps the pre-existing result for an empty list, where no row can be picked", () => {
    expect(arrowSelectionStep("ArrowDown", 0, 0)).toBe(-1);
    expect(arrowSelectionStep("ArrowUp", 0, 0)).toBe(0);
  });
});
