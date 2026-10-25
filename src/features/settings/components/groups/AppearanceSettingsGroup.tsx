import NativeFeatureBoundary from "../NativeFeatureBoundary";
import type { ComponentType, ReactNode, CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, message } from "@tauri-apps/plugin-dialog";
import { ChevronDown, ChevronRight, X } from "lucide-react";
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

const clampProgress = (value: number, min: number, max: number) => {
    if (max <= min) return "0%";
    const pct = ((value - min) / (max - min)) * 100;
    return `${Math.min(100, Math.max(0, pct))}%`;
};

const buildRangeStyle = (value: number, min: number, max: number) =>
    ({
        width: '100%',
        cursor: 'pointer',
        accentColor: 'var(--accent-color)',
        "--range-progress": clampProgress(value, min, max)
    }) as CSSProperties;

type SystemFont = { family: string; is_mono: boolean; path: string };

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
    const [previewSaving, setPreviewSaving] = useState(false);
    const [previewSaveError, setPreviewSaveError] = useState(false);
    const [draftPreviewOpacity, setDraftPreviewOpacity] = useState<number | null>(null);
    const previewSaveVersion = useRef(0);
    const savePreview = async (save: () => Promise<void>) => {
        const request = ++previewSaveVersion.current;
        setPreviewSaving(true);
        setPreviewSaveError(false);
        try { await save(); }
        catch { if (request === previewSaveVersion.current) setPreviewSaveError(true); }
        finally {
            await reloadPreviewPreferences();
            if (request === previewSaveVersion.current) {
                setPreviewSaving(false);
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
    const fontMainOptions = useMemo(() => [
        defaultOption,
        ...systemFonts.filter(f => !f.is_mono).map(f => ({ value: f.family, label: f.family }))
    ], [defaultOption, systemFonts]);
    const fontMonoOptions = useMemo(() => [
        defaultOption,
        ...systemFonts.filter(f => f.is_mono).map(f => ({ value: f.family, label: f.family }))
    ], [defaultOption, systemFonts]);

    return (
    <div className={`settings-group ${collapsed ? 'collapsed' : ''}`}>
        <div className="group-header" onClick={onToggle}>
            <h3 style={{ margin: 0 }}>{t('appearance_settings')}</h3>
            {collapsed ? <ChevronRight size={16} /> : <ChevronDown size={16} />}
        </div>
        {!collapsed && (
            <div className="group-content">
                <div className="setting-item column">
                    <ThemeManager t={t} theme={theme} setTheme={setTheme} saveAppSetting={saveAppSetting} />
                </div>

                <div className="setting-item column">
                    <div className="item-label-group" style={{ marginBottom: '8px' }}>
                        <span className="item-label">{t('color_mode')}</span>
                    </div>
                    <div style={{ display: 'flex', gap: '8px', width: '100%' }}>
                        {[
                            { id: 'system', name: t('mode_system') },
                            { id: 'light', name: t('mode_light') },
                            { id: 'dark', name: t('mode_dark') }
                        ].map(modeItem => (
                            <button
                                key={modeItem.id}
                                onClick={() => {
                                    setColorMode(modeItem.id);
                                    saveAppSetting('color_mode', modeItem.id);
                                }}
                                className={`btn-icon ${colorMode === modeItem.id ? 'active' : ''}`}
                                style={{ flex: 1, height: '36px', fontSize: '12px', fontWeight: 'bold' }}
                            >
                                {modeItem.name}
                            </button>
                        ))}
                    </div>
                </div>

                <div className="setting-item column no-border">
                    <div className="item-label-group" style={{ marginBottom: '8px' }}>
                        <span className="item-label">{t('language')}</span>
                    </div>
                    <div style={{ display: 'flex', gap: '8px', width: '100%' }}>
                        {[
                            { id: 'zh', name: '简体' },
                            { id: 'tw', name: '繁體' },
                            { id: 'en', name: 'English' }
                        ].map(lang => (
                            <button
                                key={lang.id}
                                onClick={() => {
                                    setLanguage(lang.id as Locale);
                                    saveAppSetting('language', lang.id);
                                }}
                                className={`btn-icon ${language === lang.id ? 'active' : ''}`}
                                style={{ flex: 1, height: '36px', fontSize: '12px', fontWeight: 'bold' }}
                            >
                                {lang.name}
                            </button>
                        ))}
                    </div>
                </div>

                <div className="setting-item">
                    <LabelWithHint
                        label={t('show_app_border') || '显示应用边框'}
                        hintKey="show_app_border"
                    />
                    <label className="switch">
                        <input
                            className="cb"
                            type="checkbox"
                            checked={showAppBorder}
                            onChange={(e) => {
                                const val = e.target.checked;
                                setShowAppBorder(val);
                                saveAppSetting('show_app_border', String(val));
                            }}
                        />
                        <div className="toggle"><div className="left" /><div className="right" /></div>
                    </label>
                </div>

                <div className="setting-item">
                    <LabelWithHint
                        label={t('compact_mode') || 'Compact Mode'}
                        hintKey="compact_mode"
                    />
                    <label className="switch">
                        <input
                            className="cb"
                            type="checkbox"
                            checked={compactMode}
                            onChange={(e) => {
                                const val = e.target.checked;
                                setCompactMode(val);
                                saveAppSetting('compact_mode', String(val));
                            }}
                        />
                        <div className="toggle"><div className="left" /><div className="right" /></div>
                    </label>
                </div>

                <div className="setting-item column">
                    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '8px' }}>
                        <LabelWithHint
                            label={t('clipboard_item_font_size') || '条目字体大小'}
                            hintKey="clipboard_item_font_size"
                        />
                        <span className="hint" style={{ fontSize: '10px', color: 'var(--text-secondary)', whiteSpace: 'nowrap' }}>
                            {clipboardItemFontSize}px
                        </span>
                    </div>
                    <input
                        type="range"
                        min="11"
                        max="18"
                        step="1"
                        value={clipboardItemFontSize}
                        onChange={(e) => {
                            const val = parseInt(e.target.value);
                            setClipboardItemFontSize(val);
                            saveAppSetting('clipboard_item_font_size', String(val));
                        }}
                        style={buildRangeStyle(clipboardItemFontSize, 11, 18)}
                    />
                </div>

                <div className="setting-item column">
                    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '8px' }}>
                        <LabelWithHint
                            label={t('clipboard_tag_font_size') || '标签字体大小'}
                            hintKey="clipboard_tag_font_size"
                        />
                        <span className="hint" style={{ fontSize: '10px', color: 'var(--text-secondary)', whiteSpace: 'nowrap' }}>
                            {clipboardTagFontSize}px
                        </span>
                    </div>
                    <input
                        type="range"
                        min="8"
                        max="14"
                        step="1"
                        value={clipboardTagFontSize}
                        onChange={(e) => {
                            const val = parseInt(e.target.value);
                            setClipboardTagFontSize(val);
                            saveAppSetting('clipboard_tag_font_size', String(val));
                        }}
                        style={buildRangeStyle(clipboardTagFontSize, 8, 14)}
                    />
                </div>

                <div className="setting-item column">
                    <LabelWithHint
                        label={t('font_main') || '界面字体'}
                        hintKey="font_main"
                    />
                    <ThemedSelect
                        options={fontMainOptions}
                        value={fontMain}
                        width="100%"
                        searchable
                        noOptionsMessage={t('no_matching_fonts') || '无匹配字体'}
                        onChange={(val) => {
                            setFontMain(val);
                            saveAppSetting('font_main', val);
                        }}
                    />
                </div>

                <div className="setting-item column">
                    <LabelWithHint
                        label={t('font_mono') || '等宽字体'}
                        hintKey="font_mono"
                    />
                    <ThemedSelect
                        options={fontMonoOptions}
                        value={fontMono}
                        width="100%"
                        searchable
                        noOptionsMessage={t('no_matching_fonts') || '无匹配字体'}
                        onChange={(val) => {
                            setFontMono(val);
                            saveAppSetting('font_mono', val);
                        }}
                    />
                </div>

                <h4 className="settings-section-title">{t("preview_settings")}</h4>
                <NativeFeatureBoundary t={t}>
                    <div className="setting-item">
                        <div className="item-label-group">
                            <label className="item-label" htmlFor="preview-keyboard-enabled">{t("preview_keyboard_enabled")}</label>
                            <span className="hint">{t("preview_keyboard_enabled_hint")}</span>
                        </div>
                        <label className="switch">
                            <input id="preview-keyboard-enabled" className="cb" type="checkbox"
                                checked={previewPreferences.keyboardEnabled} disabled={!previewReady || previewSaving}
                                onChange={event => { const value = event.target.checked; void savePreview(() => savePreviewPreference("keyboardEnabled", value)); }} />
                            <div className="toggle"><div className="left" /><div className="right" /></div>
                        </label>
                    </div>
                    <div className="setting-item column">
                        <div className="item-label-group">
                            <label className="item-label" htmlFor="preview-opacity">{t("preview_opacity")}</label>
                            <span className="hint">{draftPreviewOpacity ?? previewPreferences.opacity}%</span>
                        </div>
                        <input id="preview-opacity" type="range" min="0" max="100"
                            aria-label={t("preview_opacity")} value={draftPreviewOpacity ?? previewPreferences.opacity}
                            disabled={!previewReady}
                            onChange={event => {
                                const value = Number(event.target.value);
                                setDraftPreviewOpacity(value);
                                void savePreview(() => savePreviewPreference("opacity", value));
                            }} style={buildRangeStyle(draftPreviewOpacity ?? previewPreferences.opacity, 0, 100)} />
                        <span className="hint">{t("preview_opacity_hint")}</span>
                    </div>
                    {(previewSaveError || previewLoadError) && <p className="hint" role="alert">{t("preview_preferences_failed")}</p>}
                </NativeFeatureBoundary>

                {(['mica', 'acrylic', 'liquid-glass'].includes(resolveBuiltinTheme(theme))) && (
                    <>
                        <div className="setting-item column no-border">
                            <div className="item-label-group" style={{ marginBottom: '8px' }}>
                                <span className="item-label">{t('custom_background') || '自定义背景'}</span>
                            </div>
                            <div style={{ display: 'flex', gap: '8px', width: '100%', alignItems: 'center' }}>
                                <button
                                    onClick={async () => {
                                        try {
                                            const selected = await open({
                                                multiple: false,
                                                filters: [{
                                                    name: 'Image',
                                                    extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif']
                                                }]
                                            });
                                            if (selected && typeof selected === 'string') {
                                                try {
                                                    const stats = await invoke<{ size: number }>('get_file_size', { path: selected });
                                                    const maxSize = 10 * 1024 * 1024;
                                                    if (stats.size > maxSize) {
                                                        await message(
                                                            t('background_size_error') || `图片文件过大！请选择小于 ${Math.round(maxSize / 1024 / 1024)}MB 的图片。`,
                                                            { title: t('error') || '错误', kind: 'error' }
                                                        );
                                                        return;
                                                    }
                                                } catch (e) { console.warn(e); }
                                                setCustomBackground(selected);
                                                saveAppSetting('custom_background', selected);
                                            }
                                        } catch (err) { console.error(err); }
                                    }}
                                    className="btn-icon"
                                    style={{ flex: 1, height: '36px', fontSize: '12px', fontWeight: 'bold' }}
                                >
                                    {customBackground ? (t('change_background') || '更换背景') : (t('choose_background') || '选择背景')}
                                </button>
                                {customBackground && (
                                    <button
                                        onClick={() => {
                                            setCustomBackground('');
                                            saveAppSetting('custom_background', '');
                                        }}
                                        className="btn-icon"
                                        style={{ height: '36px', fontSize: '12px', fontWeight: 'bold', padding: '0 12px' }}
                                        title={t('clear_background') || '清除背景'}
                                    >
                                        <X size={16} />
                                    </button>
                                )}
                            </div>
                            {customBackground && (
                                <div style={{ fontSize: '11px', color: 'var(--text-secondary)', marginTop: '4px', wordBreak: 'break-all' }}>
                                    {customBackground.split(/[/\\]/).pop()}
                                </div>
                            )}

                            {customBackground && (
                                <div className="setting-item column no-border" style={{ marginTop: '12px' }}>
                                    <div className="item-label-group" style={{ marginBottom: '4px', flexDirection: 'row', justifyContent: 'space-between', alignItems: 'center' }}>
                                        <span className="item-label">{t('background_opacity')}</span>
                                        <span className="hint" style={{ fontSize: '10px', color: 'var(--text-secondary)' }}>{customBackgroundOpacity}%</span>
                                    </div>
                                    <input
                                        type="range"
                                        min="0"
                                        max="100"
                                        value={customBackgroundOpacity}
                                        onChange={(e) => {
                                            const val = parseInt(e.target.value);
                                            setCustomBackgroundOpacity(val);
                                            saveAppSetting('custom_background_opacity', String(val));
                                        }}
                                        style={buildRangeStyle(customBackgroundOpacity, 0, 100)}
                                    />
                                </div>
                            )}
                        </div>

                        <NativeFeatureBoundary t={t}>
                        <div className="setting-item column">
                            <div className="item-label-group" style={{ marginBottom: '4px', flexDirection: 'row', justifyContent: 'space-between', alignItems: 'center' }}>
                                <span className="item-label">{t('surface_opacity') || '界面底板透明度'}</span>
                                <span className="hint" style={{ fontSize: '10px', color: 'var(--text-secondary)' }}>{surfaceOpacity}%</span>
                            </div>
                            <input
                                type="range"
                                min="0"
                                max="100"
                                value={surfaceOpacity}
                                onChange={(e) => {
                                    const val = parseInt(e.target.value);
                                    setSurfaceOpacity(val);
                                    saveAppSetting('surface_opacity', String(val));
                                }}
                                style={buildRangeStyle(surfaceOpacity, 0, 100)}
                            />

                        </div>
                        </NativeFeatureBoundary>
                    </>
                )}
            </div>
        )}
    </div>
);
};

export default AppearanceSettingsGroup;
