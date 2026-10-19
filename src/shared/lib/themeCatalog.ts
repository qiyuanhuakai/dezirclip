import { parseThemeCatalog, type ThemePackage } from "./themePackage";

const CACHE_KEY = "dezirclip_custom_themes_v1";
let themes: ThemePackage[] = [];
const listeners = new Set<() => void>();

export const getCustomThemes = (): readonly ThemePackage[] => themes;
export const getCustomTheme = (theme: string): ThemePackage | undefined =>
  theme.startsWith("custom:") ? themes.find(item => item.id === theme.slice(7)) : undefined;

export const replaceCustomThemes = (input: unknown): void => {
  const next = parseThemeCatalog(input);
  if (JSON.stringify(next) === JSON.stringify(themes)) return;
  themes = next;
  try { localStorage.setItem(CACHE_KEY, JSON.stringify(themes)); } catch { /* Storage may be unavailable. */ }
  for (const listener of listeners) listener();
};

export const loadCustomThemesFromSettings = (settings: Record<string, string>): void => {
  try { replaceCustomThemes(JSON.parse(settings["app.custom_themes"] || "[]") as unknown); }
  catch (error) {
    console.warn("[theme] Invalid stored theme catalog", error);
    replaceCustomThemes([]);
  }
};

export const loadCachedCustomThemes = (): void => {
  try { replaceCustomThemes(JSON.parse(localStorage.getItem(CACHE_KEY) || "[]") as unknown); }
  catch {
    try { localStorage.removeItem(CACHE_KEY); } catch { /* Storage may be unavailable. */ }
    replaceCustomThemes([]);
  }
};

export const subscribeCustomThemes = (listener: () => void): (() => void) => {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
};
