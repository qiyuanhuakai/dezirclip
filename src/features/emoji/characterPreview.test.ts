import { describe, expect, it } from "vitest";
import catalogData from "../../../frontends/dzc-surface/cmd/dzc-desktop/emoji-data.json";
import { createCharacterSearch } from "./characterCatalog";
import { emojiPreview, hasSymbolPreview, symbolPreview } from "./characterPreview";

const searchCharacters = createCharacterSearch(catalogData);

describe("character previews", () => {
  it("has offline artwork for every flag in the catalog", () => {
    const flags = searchCharacters("emoji", "").find(group => group.id === "flags")!.entries;
    expect(flags.length).toBeGreaterThan(250);
    for (const { value } of flags) expect(emojiPreview(value)?.type).toBe("sprite");
  });

  it("has glyphs for Greek letters, music and math", () => {
    for (const glyph of ["π", "𝄞", "⩽"]) {
      expect(hasSymbolPreview(glyph)).toBe(true);
      expect(symbolPreview(glyph).type).toBe("font");
    }
  });

  it("keeps symbols with unavailable glyphs in search", () => {
    const value = "\u{1CC00}";
    expect(hasSymbolPreview(value)).toBe(false);
    expect(symbolPreview(value)).toEqual({ type: "code", code: "U+1CC00" });
    const matches = searchCharacters("symbols", "U+1CC00").flatMap(group => group.entries);
    expect(matches.some(entry => entry.value === value)).toBe(true);
  });
});
