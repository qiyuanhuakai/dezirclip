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

export const applyThemeClass = (root: HTMLElement, body: HTMLElement, theme: string) => {
  clearThemeClasses(root);
  clearThemeClasses(body);
  root.classList.add(`theme-${theme}`);
  body.classList.add(`theme-${theme}`);
};

export const applyModeClass = (root: HTMLElement, body: HTMLElement, mode: ThemeMode) => {
  root.classList.remove("light-mode", "dark-mode");
  body.classList.remove("light-mode", "dark-mode");
  root.classList.add(mode === "dark" ? "dark-mode" : "light-mode");
  body.classList.add(mode === "dark" ? "dark-mode" : "light-mode");
};

export const resolveThemeMode = (colorMode: ThemeColorMode, systemIsDark: boolean): ThemeMode => {
  if (colorMode === "dark") return "dark";
  if (colorMode === "light") return "light";
  return systemIsDark ? "dark" : "light";
};
