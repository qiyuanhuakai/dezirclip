import { useEffect, useState, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import { getCustomTheme, getCustomThemes, replaceCustomThemes, subscribeCustomThemes } from "../../../shared/lib/themeCatalog";
import { BUILTIN_THEMES, MAX_CUSTOM_THEMES, parseThemePackage } from "../../../shared/lib/themePackage";
import "../../../styles/components/theme-manager.css";
import { errorMessage } from "../../../shared/lib/errorMessage";

type Props = {
  t: (key: string) => string;
  theme: string;
  setTheme: (theme: string) => void;
  saveAppSetting: (key: string, value: string) => void;
};
const themeLabel = (theme: string, t: Props["t"]) => {
  const key = `theme_${theme}`;
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
  return <section className="theme-manager" aria-label={t("visual_theme")} aria-busy={busy}>
    <div className="theme-manager-heading">
      <span className="item-label">{t("visual_theme")}</span>
      <div className="theme-manager-actions">
        <button type="button" className="setting-btn" disabled={busy || themes.length >= MAX_CUSTOM_THEMES} onClick={() => void run(async () => {
          const path = await open({ multiple: false, filters: [{ name: "DezirClip Theme", extensions: ["json"] }] });
          if (typeof path !== "string") return;
          const imported = parseThemePackage(await invoke<unknown>("import_custom_theme", { path }));
          await refreshThemes();
          choose(`custom:${imported.id}`);
          setNotice(t("theme_imported"));
        })}>{t("theme_import")}</button>
      </div>
    </div>
    <div className="theme-choices" role="group" aria-label={t("theme_builtin")}>
      {BUILTIN_THEMES.map(id => <button type="button" className={`btn-icon ${theme === id ? "active" : ""}`} style={{ width: "100%", height: "36px", fontSize: "12px", fontWeight: "bold" }} key={id} aria-pressed={theme === id} disabled={busy} onClick={() => choose(id)}>{themeLabel(id, t)}</button>)}
    </div>
    {themes.length > 0 && <div className="theme-custom-list" role="group" aria-label={t("theme_custom")}>
      {themes.map(item => <button type="button" className={`btn-icon theme-custom-choice ${theme === `custom:${item.id}` ? "active" : ""}`} key={item.id} aria-pressed={theme === `custom:${item.id}`} disabled={busy} onClick={() => choose(`custom:${item.id}`)}>
        {item.name}
      </button>)}
    </div>}
    {selected && <div className="theme-manager-actions" aria-label={selected.name}>
      <button type="button" className="setting-btn" disabled={busy} onClick={() => void run(async () => {
        const path = await save({ defaultPath: `${selected.id}.json`, filters: [{ name: "DezirClip Theme", extensions: ["json"] }] });
        if (!path) return;
        await invoke("export_custom_theme", { id: selected.id, path });
        setNotice(t("theme_exported"));
      })}>{t("theme_export")}</button>
      <button type="button" className="setting-btn" disabled={busy} onClick={() => void run(async () => {
        if (!await ask(t("theme_delete_confirm"), { title: selected.name, kind: "warning" })) return;
        await invoke("delete_custom_theme", { id: selected.id });
        await refreshThemes();
        if (theme === `custom:${selected.id}`) choose("mica");
      })}>{t("theme_delete")}</button>
    </div>}
    {themes.length >= MAX_CUSTOM_THEMES && <p className="theme-manager-hint">{t("theme_limit_reached")}</p>}

    {error && <p className="theme-operation-error" role="alert">{error}</p>}
    {notice && <p className="theme-manager-hint" role="status">{notice}</p>}
  </section>;
};

export default ThemeManager;
