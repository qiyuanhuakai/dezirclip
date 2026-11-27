import { describe, expect, it } from "vitest";
import catalogData from "../../../assets/catalogs/emoji.json";
import { createCharacterSearch } from "./characterCatalog";

const searchCharacters = createCharacterSearch(catalogData);

describe("searchCharacters", () => {
  it.each([
    ["emoji", "笑脸", "😀"],
    ["emoji", "愛心", "❤️"],
    ["emoji", ":grinning:", "😀"],
    ["emoji", "❤", "❤️"],
    ["symbols", "U+2A7D", "⩽"],
    ["symbols", "slanted equal", "⩽"],
    ["symbols", "左右雙箭頭", "⇔"],
    ["symbols", "約等", "≈"],
    ["symbols", "平方根", "√"],
    ["symbols", "向右的虛線箭頭", "⇢"],
    ["symbols", "向右的虛箭頭", "⇢"],
    ["emoji", "笑到打滕", "🤣"],
    ["symbols", "段落符號", "¶"],
    ["symbols", "圆周率", "π"],
    ["symbols", "slnted equal", "⩽"],
    ["symbols", "inifinity", "∞"],
    ["symbols", "ＩＮＦＩＮＩＴＹ", "∞"],
    ["symbols", ":", ":"],
    ["emoji", "笑到打滾", "🤣"],
    ["emoji", "puppy eyes", "🥺"],
    ["emoji", "buterfly", "🦋"],
    ["emoji", "点赞", "👍"],
    ["emoji", "比個心", "🫶"],
    ["emoji", "中国国旗", "🇨🇳"],
    ["emoji", "英國旗幟", "🇬🇧"],
    ["emoji", "EU flag", "🇪🇺"],
  ] as const)("finds %s for %s", (tab, query, value) => {
    const entries = searchCharacters(tab, query).flatMap(group => group.entries);
    expect(entries.map(entry => entry.value)).toContain(value);
  });

  it("puts practical names ahead of less specific results", () => {
    expect(searchCharacters("symbols", "打勾")[0]?.entries[0]?.value).toBe("✓");
    expect(searchCharacters("emoji", "笑到哭")[0]?.entries[0]?.value).toBe("😂");
  });

  it("keeps emoji presentation out of the standalone symbol results", () => {
    expect(searchCharacters("symbols", "😀")).toEqual([]);
    expect(searchCharacters("symbols", "no symbol matches this exact phrase")).toEqual([]);
  });
});
