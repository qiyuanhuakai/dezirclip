export type ThemeMode = "light" | "dark";
export type ThemeColorMode = "light" | "dark" | "system";

const themeCssLoaders = import.meta.glob("../../styles/themes/*.css");
const loadedThemes = new Set<string>();

export const ensureThemeCssLoaded = async (theme: string) => {
  if (!theme || loadedThemes.has(theme)) return;
  const loader = themeCssLoaders[`../../styles/themes/${theme}.css`];
  if (!loader) return;
  await loader();
  loadedThemes.add(theme);
};

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
