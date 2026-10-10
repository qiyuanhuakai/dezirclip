export type ThemeMode = "light" | "dark";
export type ThemeColorMode = "light" | "dark" | "system";

const themeCssLoaders = import.meta.glob("../../styles/themes/*.css");
const loadedThemes = new Set<string>();

/** What the app falls back to before a theme has ever been chosen. */
const DEFAULT_THEME = "mica";

export const ensureThemeCssLoaded = async (theme: string) => {
  if (!theme || loadedThemes.has(theme)) return;
  const loader = themeCssLoaders[`../../styles/themes/${theme}.css`];
  if (!loader) return;
  await loader();
  loadedThemes.add(theme);
};

/**
 * The OS preference, read synchronously.
 *
 * It is the cheapest of the three answers `systemTheme.ts` can give and the
 * only one available before the first render, which is why the boot path below
 * uses it. The native and window reads stay authoritative afterwards and
 * correct it if the two ever disagree.
 */
export const mediaSystemIsDark = (): boolean =>
  !!(window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches);

/** Coerce a stored colour mode to a known value; anything unknown follows the OS. */
export const normalizeColorMode = (raw: string): ThemeColorMode =>
  raw === "light" || raw === "dark" ? raw : "system";

const clearThemeClasses = (element: HTMLElement) => {
  Array.from(element.classList)
    .filter((className) => className.startsWith("theme-"))
    .forEach((className) => element.classList.remove(className));
};

const hasOnly = (element: HTMLElement, wanted: string, prefix: string) =>
  element.classList.contains(wanted) &&
  !Array.from(element.classList).some((name) => name !== wanted && name.startsWith(prefix));

export const applyThemeClass = (root: HTMLElement, body: HTMLElement, theme: string) => {
  const next = `theme-${theme}`;
  // Every classList write on <html>/<body> invalidates style for the whole
  // document, so converge only when the DOM is not already in the target state.
  if (hasOnly(root, next, "theme-") && hasOnly(body, next, "theme-")) return;
  clearThemeClasses(root);
  clearThemeClasses(body);
  root.classList.add(next);
  body.classList.add(next);
};

export const applyModeClass = (root: HTMLElement, body: HTMLElement, mode: ThemeMode) => {
  const next = mode === "dark" ? "dark-mode" : "light-mode";
  const other = next === "dark-mode" ? "light-mode" : "dark-mode";
  // This runs on every tick of the 2s system-theme poll, and each write on
  // <html>/<body> re-resolves every theme rule in the document. Skipping the
  // no-op case keeps a following-the-OS window from recalculating constantly.
  const settled = (element: HTMLElement) =>
    element.classList.contains(next) && !element.classList.contains(other);
  if (settled(root) && settled(body)) return;
  root.classList.remove("light-mode", "dark-mode");
  body.classList.remove("light-mode", "dark-mode");
  root.classList.add(next);
  body.classList.add(next);
};

export const resolveThemeMode = (colorMode: ThemeColorMode, systemIsDark: boolean): ThemeMode => {
  if (colorMode === "dark") return "dark";
  if (colorMode === "light") return "light";
  return systemIsDark ? "dark" : "light";
};

const readBootTheme = (): string => {
  try {
    return localStorage.getItem("dezirclip_theme") || localStorage.getItem("tiez_theme") || DEFAULT_THEME;
  } catch {
    return DEFAULT_THEME;
  }
};

const readBootColorMode = (): string => {
  try {
    return localStorage.getItem("dezirclip_color_mode") || "";
  } catch {
    return "";
  }
};

const hasThemeSheet = (theme: string): boolean =>
  Boolean(themeCssLoaders[`../../styles/themes/${theme}.css`]);

/**
 * Put the saved theme on screen before the app has rendered anything.
 *
 * The theme is already known this early -- localStorage is synchronous, and
 * `App.tsx` writes to it the moment the setting changes -- but nothing was
 * using it that early. The stylesheet was fetched and then dropped, and the
 * `theme-*` class that scopes every rule inside it only arrived much later:
 * after `App` had loaded, mounted, and round-tripped `get_settings`. Measured on
 * this machine the sheet was in hand ~46 ms into a boot while the class landed
 * at ~163 ms, so a recreated webview spent its first ~150 ms painted in the
 * base stylesheet instead of the user's theme.
 *
 * Writing the class here closes that gap, and writing only the class is what
 * matters: merging the stylesheet into the blocking one was measured too and
 * buys nothing, because the sheet was never the thing that arrived late.
 *
 * Both classes are written because the appearance is both of them. A stored
 * explicit light/dark needs no system read at all; only `system` falls back to
 * the media query, and `useSettingsApply` still corrects that afterwards if the
 * native read disagrees.
 */
export const applyBootAppearance = () => {
  const theme = readBootTheme();
  const mode = resolveThemeMode(normalizeColorMode(readBootColorMode()), mediaSystemIsDark());
  const root = document.documentElement;
  const body = document.body;
  // main.tsx runs as a deferred module script, so the document is parsed and
  // both elements exist by the time this is called.
  applyThemeClass(root, body, theme);
  applyModeClass(root, body, mode);

  // Deliberately not awaited: the fetch starts now and has normally finished
  // before React commits. Blocking the first paint on 62 KB of theme CSS would
  // trade a cosmetic gap for a slower window, on the largest themes only.
  void ensureThemeCssLoaded(hasThemeSheet(theme) ? theme : DEFAULT_THEME);
};
