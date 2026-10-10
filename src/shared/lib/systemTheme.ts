import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { ThemeColorMode } from "./themeRuntime";

// Colour mode resolution shared by the main window and the auxiliary windows.
// The native read is authoritative on Windows (where DWM owns the theme), the
// window read covers Linux, and the media query is the last resort.

export const readNativeSystemIsDark = async (): Promise<boolean | null> => {
  try {
    const mode = await invoke<string>("get_system_theme_mode");
    if (mode === "dark") return true;
    if (mode === "light") return false;
  } catch {
    // Fall through to the Web media detection below.
  }
  return null;
};

export const readWindowSystemIsDark = async (): Promise<boolean | null> => {
  try {
    return (await getCurrentWindow().theme()) === "dark";
  } catch {
    return null;
  }
};

export const mediaSystemIsDark = (): boolean =>
  !!(window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches);

export const readSystemIsDark = async (): Promise<boolean> =>
  (await readNativeSystemIsDark()) ?? (await readWindowSystemIsDark()) ?? mediaSystemIsDark();

/** Coerce a stored colour mode to a known value; anything unknown follows the OS. */
export const normalizeColorMode = (raw: string): ThemeColorMode =>
  raw === "light" || raw === "dark" ? raw : "system";

/** True when the app is following the OS theme and must re-resolve on change. */
export const followsSystemTheme = (colorMode: ThemeColorMode): boolean => colorMode === "system";
