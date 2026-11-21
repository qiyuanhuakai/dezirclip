import { SettingsToggleField } from "@deziro/ui-react/components/SettingsToggle";
import { SettingsField } from "@deziro/ui-react/components/SettingsField";
import { SettingsSlider } from "@deziro/ui-react/components/SettingsSlider";
import { SettingsBackground } from "@deziro/ui-react/components/SettingsAppearance";
import { SettingHelp } from "../SettingHelp";
import NativeFeatureBoundary from "../NativeFeatureBoundary";
import type { ComponentType, ReactNode } from "react";
import { invoke } from "../../../../shared/lib/desktopInvoke";
import { open, message } from "@tauri-apps/plugin-dialog";
import { SettingsGroupHeader } from "@deziro/ui-react/components/SettingsSectionCard";
import { SettingsChoices } from "@deziro/ui-react/components/SettingsChoices";
import type { Locale } from "../../../../shared/types";
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import ThemedSelect from "../ThemedSelect";
import { shouldRequestSystemFonts } from "../../utils/systemFontLoading";
import ThemeManager from "../ThemeManager";
import { resolveBuiltinTheme } from "../../../../shared/lib/themeRuntime";
import { getCustomThemes, subscribeCustomThemes } from "../../../../shared/lib/themeCatalog";
import { usePreviewPreferences } from "../../../../shared/hooks/usePreviewPreferences";
import { savePreviewPreference } from "../../../../shared/lib/previewPreferenceClient";

interface LabelWithHintProps {
    label: string;
    hint?: string | ReactNode;
    hintKey: string;
}

interface AppearanceSettingsGroupProps {
    t: (key: string) => string;
    collapsed: boolean;
    onToggle: () => void;
    LabelWithHint: ComponentType<LabelWithHintProps>;
    theme: string;
    setTheme: (val: string) => void;
    colorMode: string;
    setColorMode: (val: string) => void;
    language: Locale;
    setLanguage: (val: Locale) => void;
    showAppBorder: boolean;
    setShowAppBorder: (val: boolean) => void;
    compactMode: boolean;
    setCompactMode: (val: boolean) => void;
    clipboardItemFontSize: number;
    setClipboardItemFontSize: (val: number) => void;
    clipboardTagFontSize: number;
    setClipboardTagFontSize: (val: number) => void;
    fontMain: string;
    setFontMain: (val: string) => void;
    fontMono: string;
    setFontMono: (val: string) => void;
    customBackground: string;
    setCustomBackground: (val: string) => void;
    customBackgroundOpacity: number;
    setCustomBackgroundOpacity: (val: number) => void;
    surfaceOpacity: number;
    setSurfaceOpacity: (val: number) => void;
    saveAppSetting: (key: string, val: string) => void;
}

type SystemFont = { family: string; is_mono: boolean; path: string };

function fontOptions(fonts: SystemFont[], monospace: boolean, selected: string, defaultOption: { value: string; label: string }) {
    const options = [defaultOption, ...fonts.filter(font => font.is_mono === monospace)
        .map(font => ({ value: font.family, label: font.family }))];
    if (selected && !options.some(option => option.value === selected)) {
        options.push({ value: selected, label: selected });
    }
    return options;
}

// `list_system_fonts` enumerates every installed font and loads each one on the
// Rust side, with no shared cache there. This group remounts whenever the rail
// moves to another section, so a component-local `systemFontsLoaded` restarted
// the whole scan on every return visit. Holding the promise at module scope
// dedupes callers that arrive while a scan is still in flight and keeps the
// result for every later mount; a rejection clears it so the next visit retries.
let systemFontsRequest: Promise<SystemFont[]> | null = null;
const loadSystemFonts = (): Promise<SystemFont[]> => {
    systemFontsRequest ??= invoke<SystemFont[]>("list_system_fonts").catch((err: unknown) => {
        systemFontsRequest = null;
        throw err;
    });
    return systemFontsRequest;
};

const AppearanceSettingsGroup = ({
    t,
    collapsed,
    onToggle,
    LabelWithHint,
    theme,
    setTheme,
    colorMode,
    setColorMode,
    language,
    setLanguage,
    showAppBorder,
    setShowAppBorder,
    compactMode,
    setCompactMode,
    clipboardItemFontSize,
    setClipboardItemFontSize,
    clipboardTagFontSize,
    setClipboardTagFontSize,
    fontMain,
    setFontMain,
    fontMono,
    setFontMono,
    customBackground,
    setCustomBackground,
    customBackgroundOpacity,
    setCustomBackgroundOpacity,
    surfaceOpacity,
    setSurfaceOpacity,
    saveAppSetting
}: AppearanceSettingsGroupProps) => {
    useSyncExternalStore(subscribeCustomThemes, getCustomThemes);
    const { preferences: previewPreferences, ready: previewReady, error: previewLoadError, reload: reloadPreviewPreferences } = usePreviewPreferences();
    const [previewSaveError, setPreviewSaveError] = useState(false);
    const [draftPreviewOpacity, setDraftPreviewOpacity] = useState<number | null>(null);
    const previewSaveVersion = useRef(0);
    const savePreview = async (save: () => Promise<void>) => {
        const request = ++previewSaveVersion.current;
        setPreviewSaveError(false);
        try { await save(); }
        catch { if (request === previewSaveVersion.current) setPreviewSaveError(true); }
        finally {
            await reloadPreviewPreferences();
            if (request === previewSaveVersion.current) {
                setDraftPreviewOpacity(null);
            }
        }
    };
    const [systemFonts, setSystemFonts] = useState<SystemFont[]>([]);
    const [systemFontsLoaded, setSystemFontsLoaded] = useState(false);
    useEffect(() => {
        if (!shouldRequestSystemFonts(collapsed, systemFontsLoaded)) return;
        let cancelled = false;
        loadSystemFonts()
            .then((list) => {
                if (!cancelled) {
                    setSystemFonts(list);
                    setSystemFontsLoaded(true);
                }
            })
            .catch((err) => { console.warn("[font] list_system_fonts failed:", err); });
        return () => { cancelled = true; };
    }, [collapsed, systemFontsLoaded]);

    const defaultOption = useMemo(() => ({ value: "", label: t("font_default_option") }), [t]);
    const fontMainOptions = useMemo(() => fontOptions(systemFonts, false, fontMain, defaultOption), [defaultOption, systemFonts, fontMain]);
    const fontMonoOptions = useMemo(() => fontOptions(systemFonts, true, fontMono, defaultOption), [defaultOption, systemFonts, fontMono]);

    const chooseBackground = async () => {
        try {
            const selected = await open({
                multiple: false,
                filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }],
            });
            if (typeof selected !== "string") return;
            try {
                const stats = await invoke<{ size: number }>("get_file_size", { path: selected });
                if (stats.size > 10 * 1024 * 1024) {
                    await message(t("background_size_error"), { title: t("error"), kind: "error" });
                    return;
                }
            } catch (err) { console.warn(err); }
            setCustomBackground(selected);
            saveAppSetting("custom_background", selected);
        } catch (err) { console.error(err); }
    };

    return (
    <div className={`settings-group ${collapsed ? 'collapsed' : ''}`}>
        <SettingsGroupHeader title={t('appearance_settings')} collapsed={collapsed} onToggle={onToggle} />
        {!collapsed && (
            <div className="group-content">
                <SettingsField stacked>
                    <ThemeManager t={t} theme={theme} setTheme={setTheme} saveAppSetting={saveAppSetting} />
                </SettingsField>

                <SettingsField stacked>
                    <div className="item-label-group" style={{ marginBottom: '8px' }}>
                        <span className="item-label">{t('color_mode')}</span>
                    </div>
                    <SettingsChoices label={t('color_mode')} value={colorMode}
                        options={[
                            { value: 'system', label: t('mode_system') },
                            { value: 'light', label: t('mode_light') },
                            { value: 'dark', label: t('mode_dark') },
                        ]}
                        onChange={value => { setColorMode(value); saveAppSetting('color_mode', value); }} />
                </SettingsField>

                <SettingsField stacked last>
                    <div className="item-label-group" style={{ marginBottom: '8px' }}>
                        <span className="item-label">{t('language')}</span>
                    </div>
                    <SettingsChoices<Locale> label={t('language')} value={language}
                        options={[{ value: 'zh', label: '简体' }, { value: 'tw', label: '繁體' }, { value: 'en', label: 'English' }]}
                        onChange={value => { setLanguage(value); saveAppSetting('language', value); }} />
                </SettingsField>

                <SettingsToggleField label={t("show_app_border") || "显示应用边框"} helpLabel={t("settings_help")}
                    description={t("show_app_border_hint")} checked={showAppBorder}
                    onChange={(e) => {
                        const val = e.target.checked;
                        setShowAppBorder(val);
                        saveAppSetting('show_app_border', String(val));
                    }} />

                <SettingsToggleField label={t("compact_mode") || "Compact Mode"} helpLabel={t("settings_help")}
                    description={t("compact_mode_hint")} checked={compactMode}
                    onChange={(e) => {
                        const val = e.target.checked;
                        setCompactMode(val);
                        saveAppSetting('compact_mode', String(val));
                    }} />

                <SettingsField stacked>
                    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '8px' }}>
                        <LabelWithHint
                            label={t('clipboard_item_font_size') || '条目字体大小'}
                            hintKey="clipboard_item_font_size"
                        />
                        <span className="hint" style={{ fontSize: '10px', color: 'var(--text-secondary)', whiteSpace: 'nowrap' }}>
                            {clipboardItemFontSize}px
                        </span>
                    </div>
                    <SettingsSlider
                        aria-label={t('clipboard_item_font_size')}
                        min={11}
                        max={18}
                        step="1"
                        value={clipboardItemFontSize}
                        onChange={(e) => {
                            const val = parseInt(e.target.value);
                            setClipboardItemFontSize(val);
                            saveAppSetting('clipboard_item_font_size', String(val));
                        }}
                    />
                </SettingsField>

                <SettingsField stacked>
                    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '8px' }}>
                        <LabelWithHint
                            label={t('clipboard_tag_font_size') || '标签字体大小'}
                            hintKey="clipboard_tag_font_size"
                        />
                        <span className="hint" style={{ fontSize: '10px', color: 'var(--text-secondary)', whiteSpace: 'nowrap' }}>
                            {clipboardTagFontSize}px
                        </span>
                    </div>
                    <SettingsSlider
                        aria-label={t('clipboard_tag_font_size')}
                        min={8}
                        max={14}
                        step="1"
                        value={clipboardTagFontSize}
                        onChange={(e) => {
                            const val = parseInt(e.target.value);
                            setClipboardTagFontSize(val);
                            saveAppSetting('clipboard_tag_font_size', String(val));
                        }}
                    />
                </SettingsField>

                <SettingsField stacked>
                    <LabelWithHint
                        label={t('font_main') || '界面字体'}
                        hintKey="font_main"
                    />
                    <ThemedSelect
                        options={fontMainOptions}
                        ariaLabel={t('font_main')}
                        value={fontMain}
                        width="100%"
                        searchable
                        noOptionsMessage={t('no_matching_fonts') || '无匹配字体'}
                        onChange={(val) => {
                            setFontMain(val);
                            saveAppSetting('font_main', val);
                        }}
                    />
                </SettingsField>

                <SettingsField stacked>
                    <LabelWithHint
                        label={t('font_mono') || '等宽字体'}
                        hintKey="font_mono"
                    />
                    <ThemedSelect
                        options={fontMonoOptions}
                        ariaLabel={t('font_mono')}
                        value={fontMono}
                        width="100%"
                        searchable
                        noOptionsMessage={t('no_matching_fonts') || '无匹配字体'}
                        onChange={(val) => {
                            setFontMono(val);
                            saveAppSetting('font_mono', val);
                        }}
                    />
                </SettingsField>

                <NativeFeatureBoundary t={t}>
                    <SettingsField stacked>
                        <div className="settings-range-header">
                            <div className="settings-label-line">
                            <label className="item-label" htmlFor="preview-opacity">{t("preview_opacity")}</label>
                            <SettingHelp label={t("preview_opacity")}>{t("preview_opacity_hint")}</SettingHelp>
                            </div>
                            <span className="settings-range-value">{draftPreviewOpacity ?? previewPreferences.opacity}%</span>
                        </div>
                        <SettingsSlider id="preview-opacity" min={0} max={100}
                            aria-label={t("preview_opacity")} value={draftPreviewOpacity ?? previewPreferences.opacity}
                            disabled={!previewReady}
                            onChange={event => {
                                const value = Number(event.target.value);
                                setDraftPreviewOpacity(value);
                                void savePreview(() => savePreviewPreference("opacity", value));
                            }} />
                    </SettingsField>
                    {(previewSaveError || previewLoadError) && <p className="hint" role="alert">{t("preview_preferences_failed")}</p>}
                </NativeFeatureBoundary>

                {(['mica', 'acrylic', 'liquid-glass'].includes(resolveBuiltinTheme(theme))) && (
                    <>
                        <SettingsBackground label={t("custom_background")} clearLabel={t("clear_background")}
                            chooseLabel={t(customBackground ? "change_background" : "choose_background")}
                            filename={customBackground.split(/[/\\]/).pop()}
                            onChoose={() => void chooseBackground()} onClear={() => {
                                setCustomBackground("");
                                saveAppSetting("custom_background", "");
                            }}>
                            {customBackground && (
                                <div className="setting-item column no-border" style={{ marginTop: '12px' }}>
                                    <div className="settings-range-header">
                                        <span className="item-label">{t('background_opacity')}</span>
                                        <span className="settings-range-value">{customBackgroundOpacity}%</span>
                                    </div>
                                    <SettingsSlider
                                        aria-label={t('background_opacity')}
                                        min={0}
                                        max={100}
                                        value={customBackgroundOpacity}
                                        onChange={(e) => {
                                            const val = parseInt(e.target.value);
                                            setCustomBackgroundOpacity(val);
                                            saveAppSetting('custom_background_opacity', String(val));
                                        }}
                                    />
                                </div>
                            )}
                        </SettingsBackground>

                        <NativeFeatureBoundary t={t}>
                        <SettingsField stacked>
                            <div className="settings-range-header">
                                <span className="item-label">{t('surface_opacity') || '界面底板透明度'}</span>
                                <span className="settings-range-value">{surfaceOpacity}%</span>
                            </div>
                            <SettingsSlider
                                aria-label={t('surface_opacity')}
                                min={0}
                                max={100}
                                value={surfaceOpacity}
                                onChange={(e) => {
                                    const val = parseInt(e.target.value);
                                    setSurfaceOpacity(val);
                                    saveAppSetting('surface_opacity', String(val));
                                }}
                            />

                        </SettingsField>
                        </NativeFeatureBoundary>
                    </>
                )}
            </div>
        )}
    </div>
);
};

export default AppearanceSettingsGroup;
