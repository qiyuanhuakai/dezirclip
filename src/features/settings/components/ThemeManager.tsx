import { useEffect, useState, useSyncExternalStore } from "react";
import { SettingsThemeManager } from "@deziro/ui-react/components/SettingsAppearance";
import { invoke } from "../../../shared/lib/desktopInvoke";
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import { getCustomTheme, getCustomThemes, replaceCustomThemes, subscribeCustomThemes } from "../../../shared/lib/themeCatalog";
import { BUILTIN_THEMES, MAX_CUSTOM_THEMES, parseThemePackage } from "../../../shared/lib/themePackage";
import { errorMessage } from "../../../shared/lib/errorMessage";

type Props = {
  t: (key: string) => string;
  theme: string;
  setTheme: (theme: string) => void;
  saveAppSetting: (key: string, value: string) => void;
};
const themeLabel = (theme: string, t: Props["t"]) => {
  const key = `theme_${theme.replace(/-/g, "_")}`;
  const translated = t(key);
  return translated && translated !== key ? translated : theme.split("-").map(part => part.charAt(0).toUpperCase() + part.slice(1)).join(" ");
};
const refreshThemes = async () => {
  const previous = getCustomThemes();
  const result = await invoke<unknown>("get_custom_themes");
  if (previous === getCustomThemes()) replaceCustomThemes(result);
};

const ThemeManager = ({ t, theme, setTheme, saveAppSetting }: Props) => {
  const themes = useSyncExternalStore(subscribeCustomThemes, getCustomThemes);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const selected = getCustomTheme(theme);

  useEffect(() => {
    let active = true;
    refreshThemes().catch(() => { if (active) setError(t("theme_operation_failed")); });
    return () => { active = false; };
  }, [t]);

  const choose = (value: string) => {
    setTheme(value);
    saveAppSetting("theme", value);
  };
  const run = async (action: () => Promise<void>) => {
    if (busy) return;
    setBusy(true);
    setError("");
    setNotice("");
    try { await action(); }
    catch (reason) {
      const detail = errorMessage(reason);
      setError(detail ? `${t("theme_operation_failed")} ${detail}` : t("theme_operation_failed"));
    }
    finally { setBusy(false); }
  };
  const full = themes.length >= MAX_CUSTOM_THEMES;
  return <SettingsThemeManager label={t("visual_theme")} builtinLabel={t("theme_builtin")} importedLabel={t("theme_custom")}
    value={theme} disabled={busy} importDisabled={full}
    builtins={BUILTIN_THEMES.map(value => ({ value, label: themeLabel(value, t) }))}
    imported={themes.map(item => ({ value: `custom:${item.id}`, label: item.name }))}
    importLabel={t("theme_import")} exportLabel={t("theme_export")} removeLabel={t("theme_delete")}
    limitMessage={full ? t("theme_limit_reached") : undefined} error={error} notice={notice} onChange={choose}
    onImport={() => void run(async () => {
      const path = await open({ multiple: false, filters: [{ name: "DezirClip Theme", extensions: ["json"] }] });
      if (typeof path !== "string") return;
      const imported = parseThemePackage(await invoke<unknown>("import_custom_theme", { path }));
      await refreshThemes();
      choose(`custom:${imported.id}`);
      setNotice(t("theme_imported"));
    })}
    onExport={() => selected && void run(async () => {
      const path = await save({ defaultPath: `${selected.id}.json`, filters: [{ name: "DezirClip Theme", extensions: ["json"] }] });
      if (!path) return;
      await invoke("export_custom_theme", { id: selected.id, path });
      setNotice(t("theme_exported"));
    })}
    onRemove={() => selected && void run(async () => {
      if (!await ask(t("theme_delete_confirm"), { title: selected.name, kind: "warning" })) return;
      await invoke("delete_custom_theme", { id: selected.id });
      await refreshThemes();
      if (theme === `custom:${selected.id}`) choose("mica");
    })} />;
};

export default ThemeManager;
