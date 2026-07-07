import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { applyThemeClass, applyModeClass, ensureThemeCssLoaded } from "../lib/themeRuntime";

type ThemeEventDetail = { theme: string; colorMode: "light" | "dark" };
type TauriThemeEventPayload = { theme: string; color_mode: "light" | "dark" };

function readThemeFromStorage(): { theme: string; colorMode: "light" | "dark" } {
  const theme = localStorage.getItem("dezirclip_theme") || localStorage.getItem("tiez_theme") || "mica";
  const colorModeRaw = localStorage.getItem("dezirclip_color_mode") || localStorage.getItem("tiez_color_mode") || "light";
  return { theme, colorMode: colorModeRaw === "dark" ? "dark" : "light" };
}

function applyThemeFromStorage(): void {
  const { theme, colorMode } = readThemeFromStorage();
  ensureThemeCssLoaded(theme).finally(() => {
    applyThemeClass(document.documentElement, document.body, theme);
    applyModeClass(document.documentElement, document.body, colorMode);
  });
}

export function useThemeSync(): void {
  useEffect(() => {
    let disposed = false;
    applyThemeFromStorage();
    const handleStorage = (e: StorageEvent) => {
      if (
        e.key === "dezirclip_theme" ||
        e.key === "tiez_theme" ||
        e.key === "dezirclip_color_mode" ||
        e.key === "tiez_color_mode" ||
        e.key === null
      ) {
        applyThemeFromStorage();
      }
    };
    const handleCustom = (e: Event) => {
      const detail = (e as CustomEvent<ThemeEventDetail>).detail;
      if (!detail) return;
      ensureThemeCssLoaded(detail.theme).finally(() => {
        applyThemeClass(document.documentElement, document.body, detail.theme);
        applyModeClass(document.documentElement, document.body, detail.colorMode);
      });
    };
    window.addEventListener("storage", handleStorage);
    window.addEventListener("dezirclip:theme-changed", handleCustom);
    let unlistenThemeChanged: (() => void) | null = null;
    listen<TauriThemeEventPayload>("theme-changed", (event) => {
      const { theme, color_mode } = event.payload;
      ensureThemeCssLoaded(theme).finally(() => {
        if (disposed) return;
        applyThemeClass(document.documentElement, document.body, theme);
        applyModeClass(document.documentElement, document.body, color_mode);
      });
    }).then((unlisten) => {
      if (disposed) {
        unlisten();
        return;
      }
      unlistenThemeChanged = unlisten;
    }).catch(() => undefined);
    return () => {
      disposed = true;
      window.removeEventListener("storage", handleStorage);
      window.removeEventListener("dezirclip:theme-changed", handleCustom);
      unlistenThemeChanged?.();
    };
  }, []);
}
