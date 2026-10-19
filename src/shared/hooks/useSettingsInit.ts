import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Locale } from "../types";
import { getCustomThemes, loadCustomThemesFromSettings, replaceCustomThemes } from "../lib/themeCatalog";

interface UseSettingsInitOptions {
  setAppSettings: (settings: Record<string, string>) => void;
  setHotkey: (val: string) => void;
  setTheme: (val: string) => void;
  setColorMode: (val: string) => void;
  setCompactMode: (val: boolean) => void;
  setLanguage: (val: Locale) => void;
}

export const useSettingsInit = ({
  setAppSettings,
  setHotkey,
  setTheme,
  setColorMode,
  setCompactMode,
  setLanguage
}: UseSettingsInitOptions) => {
  const [settings, setSettings] = useState<Record<string, string> | null>(null);

  useEffect(() => {
    let alive = true;
    const cleanups: Array<() => void> = [];
    let loadVersion = 0;

    const loadSettings = async () => {
      const version = ++loadVersion;
      const catalog = getCustomThemes();
      try {
        const result = await invoke<Record<string, string>>("get_settings");
        if (!alive || version !== loadVersion) return;

        if (catalog === getCustomThemes()) loadCustomThemesFromSettings(result);
        setAppSettings(result);
        if (result["app.hotkey"]) setHotkey(result["app.hotkey"]);

        const loadedTheme = result["app.theme"] || "mica";
        const loadedColorMode = result["app.color_mode"] || "system";

        setTheme(loadedTheme);
        setColorMode(loadedColorMode);
        setCompactMode(result["app.compact_mode"] === "true");

        try {
          localStorage.setItem("dezirclip_theme", loadedTheme);
          localStorage.setItem("dezirclip_color_mode", loadedColorMode);
          localStorage.setItem(
            "dezirclip_compact_mode",
            result["app.compact_mode"] === "true" ? "true" : "false"
          );
        } catch {
          // Ignore localStorage errors
        }

        if (result["app.language"]) {
          setLanguage(result["app.language"] as Locale);
        }

        setSettings(result);
      } catch (e) {
        console.error(e);
      }
    };

    void loadSettings();

    (async () => {
      try {
        const off = await listen("settings-changed", () => {
          void loadSettings();
        });
        if (alive) cleanups.push(off); else off();
      } catch (e) {
        console.error(e);
      }
    })();

    void (async () => {
      try {
        const off = await listen<{ themes: unknown }>("custom-themes-changed", event => {
          if (!alive) return;
          try { replaceCustomThemes(event.payload.themes); }
          catch (error) { console.warn("[theme] Invalid theme update", error); }
        });
        if (alive) cleanups.push(off); else off();
      } catch (error) { console.warn("[theme] Failed to subscribe to theme updates", error); }
    })();

    return () => {
      alive = false;
      for (const cleanup of cleanups) cleanup();
    };
  }, [setAppSettings, setHotkey, setTheme, setColorMode, setCompactMode, setLanguage]);

  return settings;
};
