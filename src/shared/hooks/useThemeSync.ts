import { useEffect } from "react";
import { applyThemeClass, applyModeClass } from "../lib/themeRuntime";

type ThemeEventDetail = { theme: string; colorMode: "light" | "dark" };

function readThemeFromStorage(): { theme: string; colorMode: "light" | "dark" } {
  const theme = localStorage.getItem("dezirclip_theme") || localStorage.getItem("tiez_theme") || "mica";
  const colorModeRaw = localStorage.getItem("dezirclip_color_mode") || localStorage.getItem("tiez_color_mode") || "light";
  return { theme, colorMode: colorModeRaw === "dark" ? "dark" : "light" };
}

function applyThemeFromStorage(): void {
  const { theme, colorMode } = readThemeFromStorage();
  applyThemeClass(document.documentElement, document.body, theme);
  applyModeClass(document.documentElement, document.body, colorMode);
}

export function useThemeSync(): void {
  useEffect(() => {
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
      applyThemeClass(document.documentElement, document.body, detail.theme);
      applyModeClass(document.documentElement, document.body, detail.colorMode);
    };
    window.addEventListener("storage", handleStorage);
    window.addEventListener("dezirclip:theme-changed", handleCustom);
    return () => {
      window.removeEventListener("storage", handleStorage);
      window.removeEventListener("dezirclip:theme-changed", handleCustom);
    };
  }, []);
}
