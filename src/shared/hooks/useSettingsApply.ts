import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { applyModeClass, applyThemeClass, ensureThemeCssLoaded, resolveThemeMode } from "../lib/themeRuntime";
import { applyFontOverrides, applySurfaceOpacity } from "../lib/appearanceRuntime";
import { readSystemIsDark, mediaSystemIsDark } from "../lib/systemTheme";

type PlatformInfo = {
  platform: string;
  is_windows_10: boolean;
  is_windows_11: boolean;
  is_linux: boolean;
};

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
  // Last system darkness the UI was resolved to, so the poll can tell an
  // actual change from another tick of the same theme. The ref belongs to the
  // hook, not to the effect: a `useRef` called inside the effect body is a hook
  // call outside a component render, which throws "Invalid hook call" as soon
  // as the effect runs and unmounts the whole root.
  const lastSystemIsDark = useRef<boolean | null>(null);

  useEffect(() => {
    if (!settingsLoaded) return;

    const root = document.documentElement;
    const body = document.body;

    let disposed = false;

    const applyExplicitMode = (mode: "light" | "dark") => {
      if (disposed) return;
      applyModeClass(root, body, mode);
    };

    const applySystemMode = async () => {
      const isDark = await readSystemIsDark();
      if (disposed) return;
      lastSystemIsDark.current = isDark;
      applyExplicitMode(resolveThemeMode("system", isDark));
    };

    // The poll exists to catch a system theme change the two event sources
    // miss. `prefers-color-scheme` tracks the same OS preference and costs
    // nothing to read, so it is used as a gate: while it still agrees with
    // what was last applied there is nothing to re-resolve, and the native
    // read — an IPC round trip across the WebView boundary — is skipped. On a
    // platform where the media query does not follow the OS the gate never
    // holds and the poll behaves exactly as before, which is what keeps the
    // Linux WebKitGTK fallback intact.
    const systemModeChanged = () => {
      if (lastSystemIsDark.current === null) return true;
      return mediaSystemIsDark() !== lastSystemIsDark.current;
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
        if (systemModeChanged()) {
          applySystemMode();
        }
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
    applySurfaceOpacity(surfaceOpacity);
  }, [clipboardItemFontSize, clipboardTagFontSize, surfaceOpacity, settingsLoaded]);

  useEffect(() => {
    if (!settingsLoaded) return;
    applyFontOverrides(fontMain, fontMono);
  }, [fontMain, fontMono, settingsLoaded]);
};
