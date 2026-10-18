import { validateThemeCss } from "./themeCss";

export const BUILTIN_THEMES = ["retro", "mica", "acrylic", "liquid-glass", "macos", "scifi"] as const;
export type BuiltinTheme = typeof BUILTIN_THEMES[number];
export type ThemePackage = {
  format: "dezirclip-theme";
  version: 1;
  id: string;
  name: string;
  author?: string;
  description?: string;
  base: BuiltinTheme;
  css: string;
};
export const MAX_CUSTOM_THEMES = 32;
export const MAX_THEME_BYTES = 256 * 1024;
const ID_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

const object = (value: unknown): Record<string, unknown> => {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error("Theme must be an object");
  return value as Record<string, unknown>;
};
const text = (value: unknown, max: number, required: boolean, label: string): string => {
  if (typeof value !== "string") throw new Error(`${label} must be text`);
  const result = value.trim();
  if (Array.from(result).length > max || (required && !result)) throw new Error(`Invalid ${label} length`);
  if (/[\u0000-\u001f\u007f-\u009f]/.test(result) || Array.from(result).some(character => {
    const point = character.codePointAt(0) ?? 0;
    return point >= 0xd800 && point <= 0xdfff;
  })) throw new Error(`Invalid ${label} character`);
  return result;
};
const optionalText = (value: unknown, max: number, label: string): string | undefined =>
  value === undefined || value === null ? undefined : text(value, max, false, label) || undefined;
export const isBuiltinTheme = (value: string): value is BuiltinTheme => BUILTIN_THEMES.some(theme => theme === value);
export const resolveThemeSelection = (selection: string, catalog: readonly ThemePackage[]): { base: BuiltinTheme; custom?: ThemePackage } => {
  const custom = selection.startsWith("custom:") ? catalog.find(theme => theme.id === selection.slice(7)) : undefined;
  return { base: custom?.base ?? (isBuiltinTheme(selection) ? selection : "mica"), ...(custom ? { custom } : {}) };
};

export const parseThemePackage = (input: unknown): ThemePackage => {
  const value = object(input);
  if (Object.keys(value).some(key => !["format", "version", "id", "name", "author", "description", "base", "css"].includes(key))) throw new Error("Unknown theme field");
  if (value.format !== "dezirclip-theme" || value.version !== 1) throw new Error("Unsupported theme format");
  if (typeof value.id !== "string" || value.id.length > 48 || value.id.trim() !== value.id || !ID_PATTERN.test(value.id)) throw new Error("Invalid theme ID");
  if (typeof value.base !== "string" || !isBuiltinTheme(value.base)) throw new Error("Unknown base theme");
  if (typeof value.css !== "string" || new TextEncoder().encode(value.css).byteLength > MAX_THEME_BYTES) throw new Error("Invalid theme CSS size");
  validateThemeCss(value.css);
  const author = optionalText(value.author, 80, "author");
  const description = optionalText(value.description, 240, "description");
  const result: ThemePackage = {
    format: "dezirclip-theme", version: 1, id: value.id, name: text(value.name, 80, true, "name"), base: value.base,
    ...(author === undefined ? {} : { author }), ...(description === undefined ? {} : { description }), css: value.css
  };
  if (new TextEncoder().encode(JSON.stringify(result)).byteLength > MAX_THEME_BYTES) throw new Error("Theme file is too large");
  return result;
};
export const parseThemeJson = (source: string): ThemePackage => {
  if (new TextEncoder().encode(source).byteLength > MAX_THEME_BYTES) throw new Error("Theme file is too large");
  return parseThemePackage(JSON.parse(source.replace(/^\uFEFF/, "")) as unknown);
};
export const parseThemeCatalog = (input: unknown): ThemePackage[] => {
  if (!Array.isArray(input) || input.length > MAX_CUSTOM_THEMES) throw new Error("Invalid theme catalog");
  const themes = input.map(parseThemePackage);
  if (new Set(themes.map(theme => theme.id)).size !== themes.length) throw new Error("Duplicate theme ID");
  return themes;
};
