export const BUILTIN_THEMES = ["retro", "mica", "acrylic", "liquid-glass", "macos", "scifi"] as const;
export type BuiltinTheme = typeof BUILTIN_THEMES[number];
export const isBuiltinTheme = (value: string): value is BuiltinTheme => BUILTIN_THEMES.some(theme => theme === value);
export type ThemeMode = "light" | "dark";

const themeCss = {
  retro: () => import("./styles/themes/retro.css"),
  mica: () => import("./styles/themes/mica.css"),
  acrylic: () => import("./styles/themes/acrylic.css"),
  "liquid-glass": () => import("./styles/themes/liquid-glass.css"),
  macos: () => import("./styles/themes/macos.css"),
  scifi: () => import("./styles/themes/scifi.css"),
} satisfies Record<BuiltinTheme, () => Promise<unknown>>;

const loadedStyles = new Map<BuiltinTheme, Promise<void>>();

export function loadBuiltinTheme(theme: BuiltinTheme): Promise<void> {
  const loaded = loadedStyles.get(theme);
  if (loaded) return loaded;
  const pending = themeCss[theme]().then(() => {}, error => {
    loadedStyles.delete(theme);
    throw error;
  });
  loadedStyles.set(theme, pending);
  return pending;
}
