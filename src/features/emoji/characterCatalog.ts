import catalogData from "../../../frontends/dzc-surface/cmd/dzc-desktop/emoji-data.json";

interface Character {
  value: string;
  name: string;
  keywords?: string[];
}

interface CharacterGroup {
  id: string;
  entries: Character[];
}

function normalize(text: string) {
  return text.toLowerCase().replace(/[\uFE0E\uFE0F]/g, "");
}

const catalog: Record<"emoji" | "symbols", CharacterGroup[]> = catalogData;

function searchText(entry: Character) {
  const codepoints = Array.from(entry.value, value => `u+${value.codePointAt(0)!.toString(16).padStart(4, "0")}`);
  return normalize([entry.value, entry.name, ...codepoints, ...(entry.keywords ?? [])].join(" "));
}

const index = {
  emoji: catalog.emoji.map(group => group.entries.map(searchText)),
  symbols: catalog.symbols.map(group => group.entries.map(searchText)),
};

export const characterPageSize = 160;

export function searchCharacters(tab: "emoji" | "symbols", query: string) {
  const terms = normalize(query).trim().replace(/^:+|:+$/g, "").split(/\s+/).filter(Boolean);
  if (!terms.length) return catalog[tab];
  return catalog[tab].flatMap((group, groupIndex) => {
    const entries = group.entries.filter((_, indexInGroup) =>
      terms.every(term => index[tab][groupIndex][indexInGroup].includes(term)));
    return entries.length ? [{ id: group.id, entries }] : [];
  });
}
