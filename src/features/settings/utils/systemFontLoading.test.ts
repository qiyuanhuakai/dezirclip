import { describe, expect, it } from "vitest";
import { shouldRequestSystemFonts } from "./systemFontLoading";

describe("shouldRequestSystemFonts", () => {
  it("does not request fonts while the appearance group is collapsed", () => {
    expect(shouldRequestSystemFonts(true, false)).toBe(false);
  });

  it("requests fonts when appearance is expanded before fonts have loaded", () => {
    expect(shouldRequestSystemFonts(false, false)).toBe(true);
  });

  it("does not request fonts again after fonts have loaded", () => {
    expect(shouldRequestSystemFonts(false, true)).toBe(false);
  });
});
