import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { applyModeClass, applyThemeClass, resolveThemeMode } from "../lib/themeRuntime";

type PlatformInfo = {
  platform: string;
  is_windows_10: boolean;
  is_windows_11: boolean;
  is_linux: boolean;
};

const themeCssLoaders = import.meta.glob("../../styles/themes/*.css");
const loadedThemes = new Set<string>();

const ensureThemeCssLoaded = async (theme: string) => {
  if (!theme || loadedThemes.has(theme)) return;
  const path = `../../styles/themes/${theme}.css`;
  const loader = themeCssLoaders[path];
  if (!loader) return;
  await loader();
  loadedThemes.add(theme);
};

const readNativeSystemIsDark = async () => {
  try {
    const mode = await invoke<string>("get_system_theme_mode");
    if (mode === "dark") return true;
    if (mode === "light") return false;
  } catch {
    // Fall through to Tauri/Web media detection.
  }
  return null;
};

const readWindowSystemIsDark = async () => {
  try {
    return (await getCurrentWindow().theme()) === "dark";
  } catch {
    return null;
  }
};

const mediaSystemIsDark = () =>
  !!(window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches);

interface UseSettingsApplyOptions {
  theme: string;
  colorMode: string;
  showAppBorder: boolean;
  compactMode: boolean;
  settingsLoaded: boolean;
  clipboardItemFontSize: number;
  clipboardTagFontSize: number;
  surfaceOpacity: number;
  fontMain: string;
  fontMono: string;
}

export const useSettingsApply = ({
  theme,
  colorMode,
  showAppBorder,
  compactMode,
  settingsLoaded,
  clipboardItemFontSize,
  clipboardTagFontSize,
  surfaceOpacity,
  fontMain,
  fontMono
}: UseSettingsApplyOptions) => {
  useEffect(() => {
    if (!settingsLoaded) return;

    const root = document.documentElement;
    const body = document.body;

    let disposed = false;

    const applyExplicitMode = (mode: "light" | "dark") => {
      if (disposed) return;
      applyModeClass(root, body, mode);
    };

    const readSystemIsDark = async () =>
      (await readNativeSystemIsDark()) ?? (await readWindowSystemIsDark()) ?? mediaSystemIsDark();

    const applySystemMode = async () => {
      const isDark = await readSystemIsDark();
      if (disposed) return;
      applyExplicitMode(resolveThemeMode("system", isDark));
    };

    let ensureDisposed = false;
    let ensureFallbackTimer: number | null = window.setTimeout(() => {
      if (ensureDisposed) return;
      applyThemeClass(root, body, theme);
    }, 200);

    ensureThemeCssLoaded(theme)
      .catch((err) => {
        if (!ensureDisposed) console.error(err);
      })
      .finally(() => {
        if (ensureDisposed) return;
        if (ensureFallbackTimer !== null) {
          window.clearTimeout(ensureFallbackTimer);
          ensureFallbackTimer = null;
        }
        applyThemeClass(root, body, theme);
      });
    invoke<PlatformInfo>("get_platform_info")
      .then((info) => {
        if (disposed) return;
        root.classList.toggle("windows-10", !!info?.is_windows_10);
        body.classList.toggle("windows-10", !!info?.is_windows_10);
        root.classList.toggle("windows-11", !!info?.is_windows_11);
        body.classList.toggle("windows-11", !!info?.is_windows_11);
        root.classList.toggle("linux", !!info?.is_linux);
        body.classList.toggle("linux", !!info?.is_linux);
      })
      .catch(() => {
        if (disposed) return;
        root.classList.remove("windows-10", "windows-11", "linux");
        body.classList.remove("windows-10", "windows-11", "linux");
      });
    root.classList.toggle("hide-app-border", !showAppBorder);
    body.classList.toggle("hide-app-border", !showAppBorder);

    if (compactMode) {
      body.classList.add("compact-mode");
    } else {
      body.classList.remove("compact-mode");
    }

    if (colorMode === "light") {
      applyExplicitMode("light");
    } else if (colorMode === "dark") {
      applyExplicitMode("dark");
    } else {
      applySystemMode();
    }

    invoke("set_theme", {
      theme,
      color_mode: colorMode,
      show_app_border: showAppBorder
    }).catch(console.error);

    let unlistenThemeChanged: (() => void) | null = null;
    let cleanupMedia: (() => void) | null = null;
    let cleanupPoll: (() => void) | null = null;

    getCurrentWindow()
      .onThemeChanged(() => {
        if (disposed) return;

        if (colorMode === "system") {
          applySystemMode();
        } else {
          applyExplicitMode(resolveThemeMode(colorMode as "light" | "dark" | "system", false));
        }

        // Native mica/acrylic may be refreshed by the OS when system theme changes.
        // Re-apply the user's selected mode so the window background stays locked.
        invoke("set_theme", {
          theme,
          color_mode: colorMode,
          show_app_border: showAppBorder
        }).catch(console.error);
      })
      .then((f) => {
        if (disposed) {
          f();
          return;
        }
        unlistenThemeChanged = f;
      });

    if (colorMode === "system") {
      if (window.matchMedia) {
        const media = window.matchMedia("(prefers-color-scheme: dark)");
        const onChange = () => applySystemMode();
        if (media.addEventListener) {
          media.addEventListener("change", onChange);
          cleanupMedia = () => media.removeEventListener("change", onChange);
        } else {
          media.addListener(onChange);
          cleanupMedia = () => media.removeListener(onChange);
        }
      }

      const poll = window.setInterval(() => {
        applySystemMode();
      }, 2000);
      cleanupPoll = () => window.clearInterval(poll);
    }

    return () => {
      disposed = true;
      ensureDisposed = true;
      if (ensureFallbackTimer !== null) {
        window.clearTimeout(ensureFallbackTimer);
        ensureFallbackTimer = null;
      }
      if (unlistenThemeChanged) unlistenThemeChanged();
      if (cleanupMedia) cleanupMedia();
      if (cleanupPoll) cleanupPoll();
    };
  }, [theme, colorMode, showAppBorder, settingsLoaded, compactMode]);

  useEffect(() => {
    if (!settingsLoaded) return;
    const root = document.documentElement;
    root.style.setProperty("--clipboard-item-font-size", `${clipboardItemFontSize}px`);
    root.style.setProperty("--clipboard-tag-font-size", `${clipboardTagFontSize}px`);
    const scale = Math.min(1, Math.max(0, surfaceOpacity / 100));
    root.style.setProperty("--surface-opacity-scale", scale.toString());
  }, [clipboardItemFontSize, clipboardTagFontSize, surfaceOpacity, settingsLoaded]);

  useEffect(() => {
    if (!settingsLoaded) return;
    const root = document.documentElement;
    const applyFont = (cssVar: string, value: string) => {
      if (value && value.trim()) {
        root.style.setProperty(cssVar, value);
      } else {
        root.style.removeProperty(cssVar);
      }
    };
    applyFont("--font-main", fontMain);
    applyFont("--font-mono", fontMono);
  }, [fontMain, fontMono, settingsLoaded]);
};
