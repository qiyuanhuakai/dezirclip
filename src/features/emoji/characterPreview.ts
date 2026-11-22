import type { CharacterPreview } from "@deziro/ui-react/components/EmojiPicker";
import flagAtlas from "../../../frontends/dzc-surface/cmd/dzc-desktop/character-assets/flags.png";
import manifest from "../../../frontends/dzc-surface/cmd/dzc-desktop/character-assets/previews.json";
import "./characterPreview.css";

const flags = new Map(manifest.flags.map((value, index) => [value, index]));
const rows = Math.ceil(flags.size / manifest.columns);
const fonts = new Map<string, string>();
for (const { characters, family } of manifest.fonts) {
  for (const value of characters) fonts.set(value, family);
}

export function emojiPreview(value: string): CharacterPreview | undefined {
  const index = flags.get(value);
  return index === undefined ? undefined : {
    type: "sprite", src: flagAtlas, index, columns: manifest.columns, rows,
  };
}

export function hasSymbolPreview(value: string): boolean {
  return fonts.has(value);
}

export function symbolPreview(value: string): CharacterPreview {
  const family = fonts.get(value);
  if (family) return { type: "font", family };
  const code = Array.from(value, glyph => "U+" + glyph.codePointAt(0)!.toString(16).toUpperCase().padStart(4, "0")).join(" ");
  return { type: "code", code };
}
