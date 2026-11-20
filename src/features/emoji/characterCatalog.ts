import catalogData from "../../../frontends/dzc-surface/cmd/dzc-desktop/emoji-data.json";

interface Character {
  value: string;
  name: string;
  keywords?: string[];
  aliases?: string[];
}

interface CharacterGroup {
  id: string;
  entries: Character[];
}

interface SearchEntry {
  glyph: string;
  names: string[];
  aliases: string[];
  text: string;
  englishWords: string[];
  chineseNames: string[];
}

function normalize(text: string) {
  return text.normalize("NFKC").toLowerCase().replace(/[\uFE0E\uFE0F]/g, "");
}

function nameText(text: string) {
  return text.replace(/[-_–—]+/g, " ").trim().replace(/\s+/g, " ");
}

const chineseName = /^\p{Script=Han}{3,}$/u;
const catalog: Record<"emoji" | "symbols", CharacterGroup[]> = catalogData;

function indexEntry(entry: Character): SearchEntry {
  const glyph = normalize(entry.value);
  const names = [entry.name, ...(entry.keywords ?? [])].map(value => nameText(normalize(value)));
  const aliases = (entry.aliases ?? []).map(value => nameText(normalize(value)));
  const words = [...names, ...aliases];
  const codepoints = Array.from(entry.value, value => "u+" + value.codePointAt(0)!.toString(16).padStart(4, "0"));
  return { glyph, names, aliases, text: [glyph, ...words, ...codepoints].join(" "),
    englishWords: words.join(" ").match(/[a-z]+/g) ?? [], chineseNames: words.filter(word => chineseName.test(word)) };
}

const index = {
  emoji: catalog.emoji.map(group => group.entries.map(indexEntry)),
  symbols: catalog.symbols.map(group => group.entries.map(indexEntry)),
};

function oneEditApart(a: string, b: string) {
  const left = Array.from(a);
  const right = Array.from(b);
  if (Math.abs(left.length - right.length) > 1) return false;
  const shorter = left.length <= right.length ? left : right;
  const longer = left.length <= right.length ? right : left;
  let offset = 0;
  while (offset < shorter.length && shorter[offset] === longer[offset]) offset++;
  if (shorter.length !== longer.length) {
    return shorter.slice(offset).every((part, i) => part === longer[offset + i + 1]);
  }
  const remainderEquals = (start: number) => shorter.slice(start).every((part, i) => part === longer[start + i]);
  return remainderEquals(offset + 1) ||
    (shorter[offset] === longer[offset + 1] && shorter[offset + 1] === longer[offset] && remainderEquals(offset + 2));
}

export const characterPageSize = 160;

export function searchCharacters(tab: "emoji" | "symbols", query: string) {
  const literal = normalize(query).trim();
  if (!literal) return catalog[tab];
  const phrase = nameText(literal.replace(/^:([^:]+):$/, "$1")) || literal;
  const terms = phrase.split(" ");
  const typoTerms = new Map<string, "englishWords" | "chineseNames">();
  for (const term of terms) {
    if (/^[a-z]{4,}$/.test(term)) typoTerms.set(term, "englishWords");
    else if (/^\p{Script=Han}{4,}$/u.test(term)) typoTerms.set(term, "chineseNames");
  }

  const groups = (allowTypo: boolean) => catalog[tab].flatMap((group, groupIndex) => {
    const matches = group.entries.flatMap((entry, entryIndex) => {
      const candidate = index[tab][groupIndex][entryIndex];
      if (literal === candidate.glyph) return [{ entry, rank: 0 }];
      if (candidate.aliases.includes(phrase)) return [{ entry, rank: 1 }];
      if (candidate.names.includes(phrase)) return [{ entry, rank: 2 }];
      if (terms.every(term => candidate.text.includes(term))) return [{ entry, rank: 3 }];
      if (allowTypo) {
        const missing = terms.filter(term => !candidate.text.includes(term));
        const vocabulary = typoTerms.get(missing[0]);
        if (missing.length === 1 && vocabulary && candidate[vocabulary].some(word => oneEditApart(missing[0], word))) {
          return [{ entry, rank: 4 }];
        }
      }
      return [];
    }).sort((a, b) => a.rank - b.rank);
    return matches.length ? [{ id: group.id, rank: matches[0].rank, entries: matches.map(match => match.entry) }] : [];
  });

  const direct = groups(false);
  return (direct.length || !typoTerms.size ? direct : groups(true))
    .sort((a, b) => a.rank - b.rank).map(({ id, entries }) => ({ id, entries }));
}
