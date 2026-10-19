import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { applyModeClass, applyThemeClass, ensureThemeCssLoaded, resolveThemeMode } from "../lib/themeRuntime";
import type { ThemeColorMode } from "../lib/themeRuntime";
import {
  applyClipboardFontSizes,
  applyFontOverrides,
  applySurfaceOpacity
} from "../lib/appearanceRuntime";
import { followsSystemTheme, normalizeColorMode, readSystemIsDark } from "../lib/systemTheme";
import { resolveRichTextSnapshotCacheLimit } from "../lib/richTextSnapshotCache";
import { setRichTextSnapshotCacheLimit } from "../lib/richTextSnapshot";
import { getCustomThemes, loadCustomThemesFromSettings, replaceCustomThemes } from "../lib/themeCatalog";

// Backend-driven appearance for the auxiliary webviews.
//
// Every window is its own webview with its own document and its own class list,
// so nothing the main window applies is visible here. The previous approach read
// `localStorage`, which is only shared when the platform happens to give each
// webview the same storage partition — reliable on Windows, not on Linux — and
// it only covered the theme anyway. The database is the single source of truth:
// it is the same store the settings panel writes to, and it is correct even when
// an auxiliary window is opened by a global hotkey before the main window has
// ever rendered.

type PlatformInfo = {
  platform: string;
  is_windows_10: boolean;
  is_windows_11: boolean;
  is_linux: boolean;
};

type ThemeChangedPayload = { theme: string; color_mode: string };

const parseNumber = (raw: string | undefined, fallback: number): number => {
  if (raw === undefined) return fallback;
  const parsed = parseInt(raw, 10);
  return Number.isFinite(parsed) ? parsed : fallback;
};

const platformClasses = (info: PlatformInfo | null): Record<string, boolean> => ({
  "windows-10": !!info?.is_windows_10,
  "windows-11": !!info?.is_windows_11,
  linux: !!info?.is_linux
});

/**
 * Apply the parts of the appearance that do not depend on the theme, and report
 * the theme / colour mode so the caller can resolve the system mode.
 */
const applyTokens = (settings: Record<string, string>): { theme: string; colorMode: ThemeColorMode } => {
  applyFontOverrides(settings["app.font_main"] ?? "", settings["app.font_mono"] ?? "");
  applySurfaceOpacity(parseNumber(settings["app.surface_opacity"], 65));
  applyClipboardFontSizes(
    parseNumber(settings["app.clipboard_item_font_size"], 13),
    parseNumber(settings["app.clipboard_tag_font_size"], 10)
  );

  // Not appearance, but it arrives on the same channel because it has the same
  // shape of problem: each webview runs its own copy of the rich-text snapshot
  // cache, and only the compact preview renders snapshots from here. Without
  // this its Map keeps the default cap no matter what the user set, and lowering
  // the limit to free memory frees nothing in this window. `resolve` maps an
  // absent or unreadable value back to the default rather than to 0, so a
  // missing key cannot silently switch caching off.
  setRichTextSnapshotCacheLimit(
    resolveRichTextSnapshotCacheLimit(settings["app.rich_text_snapshot_cache_limit"])
  );

  const hideBorder = (settings["app.show_app_border"] ?? "true") === "false";
  const compact = (settings["app.compact_mode"] ?? "false") === "true";
  for (const el of [document.documentElement, document.body]) {
    el.classList.toggle("hide-app-border", hideBorder);
    el.classList.toggle("compact-mode", compact);
  }

  return {
    theme: settings["app.theme"] || "mica",
    colorMode: normalizeColorMode(settings["app.color_mode"] || "system")
  };
};

export const useBackendAppearance = (): void => {
  useEffect(() => {
    let disposed = false;
    let currentColorMode: ThemeColorMode = "system";
    let appearanceVersion = 0;
    let modeVersion = 0;
    const cleanups: Array<() => void> = [];

    const applyPlatformClasses = async () => {
      try {
        const info = await invoke<PlatformInfo>("get_platform_info");
        if (disposed) return;
        const classes = platformClasses(info);
        for (const el of [document.documentElement, document.body]) {
          for (const [name, on] of Object.entries(classes)) {
            el.classList.toggle(name, on);
          }
        }
      } catch (err) {
        console.warn("[appearance] Failed to read platform info:", err);
      }
    };

    const applyMode = async () => {
      if (disposed) return;
      const version = ++modeVersion;
      const mode = resolveThemeMode(currentColorMode, await readSystemIsDark());
      if (disposed || version !== modeVersion) return;
      applyModeClass(document.documentElement, document.body, mode);
    };

    const load = async () => {
      const version = ++appearanceVersion;
      const catalog = getCustomThemes();
      try {
        const settings = await invoke<Record<string, string>>("get_settings");
        if (disposed || version !== appearanceVersion) return;
        if (catalog === getCustomThemes()) loadCustomThemesFromSettings(settings ?? {});
        const { theme, colorMode } = applyTokens(settings ?? {});
        currentColorMode = colorMode;
        await applyMode();
        if (disposed || version !== appearanceVersion) return;
        await ensureThemeCssLoaded(theme);
        if (disposed || version !== appearanceVersion) return;
        applyThemeClass(document.documentElement, document.body, theme);
      } catch (err) {
        console.warn("[appearance] Failed to load appearance from backend:", err);
      }
    };

    void applyPlatformClasses();
    void load();

    // Theme switches arrive with a payload, so they need no database round-trip.
    void (async () => {
      try {
        const off = await listen<ThemeChangedPayload>("theme-changed", async (event) => {
          if (disposed) return;
          const version = ++appearanceVersion;
          const { theme, color_mode: colorMode } = event.payload;
          currentColorMode = normalizeColorMode(colorMode);
          await applyMode();
          await ensureThemeCssLoaded(theme);
          if (disposed || version !== appearanceVersion) return;
          applyThemeClass(document.documentElement, document.body, theme);
        });
        if (disposed) {
          off();
          return;
        }
        cleanups.push(off);
      } catch (err) {
        console.warn("[appearance] Failed to subscribe to theme changes:", err);
      }
    })();

    void (async () => {
      try {
        const off = await listen<{ themes: unknown }>("custom-themes-changed", event => {
          if (disposed) return;
          try { replaceCustomThemes(event.payload.themes); }
          catch (error) { console.warn("[appearance] Invalid theme update", error); }
        });
        if (disposed) off(); else cleanups.push(off);
      } catch (error) { console.warn("[appearance] Failed to observe custom themes", error); }
    })();

    // Font, shell opacity, and the clipboard font sizes have no payload channel.
    void (async () => {
      try {
        const off = await listen("appearance-changed", () => {
          void load();
        });
        if (disposed) {
          off();
          return;
        }
        cleanups.push(off);
      } catch (err) {
        console.warn("[appearance] Failed to subscribe to appearance changes:", err);
      }
    })();

    // `system` colour mode has to track the OS while the window is open.
    void (async () => {
      try {
        const unlisten = await getCurrentWindow().onThemeChanged(() => {
          if (followsSystemTheme(currentColorMode)) {
            void applyMode();
          }
        });
        if (disposed) {
          unlisten();
          return;
        }
        cleanups.push(unlisten);
      } catch (err) {
        console.warn("[appearance] Failed to observe OS theme changes:", err);
      }
    })();

    const media = window.matchMedia?.("(prefers-color-scheme: dark)");
    if (media?.addEventListener) {
      const onMediaChange = () => {
        if (followsSystemTheme(currentColorMode)) {
          void applyMode();
        }
      };
      media.addEventListener("change", onMediaChange);
      cleanups.push(() => media.removeEventListener("change", onMediaChange));
    }

    return () => {
      disposed = true;
      for (const cleanup of cleanups) {
        try {
          cleanup();
        } catch {
          // Teardown must not throw over an already-released listener.
        }
      }
    };
  }, []);
};
