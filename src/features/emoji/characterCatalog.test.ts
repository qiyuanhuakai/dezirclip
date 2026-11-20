import { describe, expect, it } from "vitest";
import { searchCharacters } from "./characterCatalog";

describe("searchCharacters", () => {
  it.each([
    ["emoji", "笑脸", "😀"],
    ["emoji", "愛心", "❤️"],
    ["emoji", ":grinning:", "😀"],
    ["emoji", "❤", "❤️"],
    ["symbols", "U+2A7D", "⩽"],
    ["symbols", "slanted equal", "⩽"],
    ["symbols", "左右雙箭頭", "⇔"],
  ] as const)("finds %s for %s", (tab, query, value) => {
    const entries = searchCharacters(tab, query).flatMap(group => group.entries);
    expect(entries.map(entry => entry.value)).toContain(value);
  });

  it("keeps emoji presentation out of the standalone symbol results", () => {
    expect(searchCharacters("symbols", "😀")).toEqual([]);
    expect(searchCharacters("symbols", "no symbol matches this exact phrase")).toEqual([]);
  });
});
