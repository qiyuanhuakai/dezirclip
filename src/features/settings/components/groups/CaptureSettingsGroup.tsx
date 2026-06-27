import { useState, useEffect, useCallback, type ComponentType, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronDown, ChevronRight, Camera } from "lucide-react";

interface LabelWithHintProps {
    label: string;
    hint?: string | ReactNode;
    hintKey: string;
}

interface CaptureSettingsGroupProps {
    t: (key: string) => string;
    collapsed: boolean;
    onToggle: () => void;
    LabelWithHint: ComponentType<LabelWithHintProps>;
    screenshotEnabled: boolean;
    setScreenshotEnabled: (val: boolean) => void;
    screenshotHotkey: string;
    quickPasteEnabled: boolean;
    setQuickPasteEnabled: (val: boolean) => void;
    quickPasteHotkey: string;
    ocrEnabled: boolean;
    setOcrEnabled: (val: boolean) => void;
    saveAppSetting: (key: string, val: string) => void;
}

const CaptureSettingsGroup = ({
    t,
    collapsed,
    onToggle,
    LabelWithHint,
    screenshotEnabled,
    setScreenshotEnabled,
    screenshotHotkey: initialScreenshotHotkey,
    quickPasteEnabled,
    setQuickPasteEnabled,
    quickPasteHotkey: initialQuickPasteHotkey,
    ocrEnabled,
    setOcrEnabled,
    saveAppSetting,
}: CaptureSettingsGroupProps) => {
    const [screenshotHotkey, setScreenshotHotkey] = useState(initialScreenshotHotkey);
    const [quickPasteHotkey, setQuickPasteHotkey] = useState(initialQuickPasteHotkey);
    const [isRecordingScreenshot, setIsRecordingScreenshot] = useState(false);
    const [isRecordingQuickPaste, setIsRecordingQuickPaste] = useState(false);
    const [isWindows, setIsWindows] = useState(false);

    useEffect(() => {
        setIsWindows(navigator.platform.startsWith("Win"));
    }, []);

    useEffect(() => {
        setScreenshotHotkey(initialScreenshotHotkey);
    }, [initialScreenshotHotkey]);

    useEffect(() => {
        setQuickPasteHotkey(initialQuickPasteHotkey);
    }, [initialQuickPasteHotkey]);

    const handleScreenshotHotkeyKeyDown = useCallback((e: React.KeyboardEvent) => {
        if (!isRecordingScreenshot) return;
        e.preventDefault();
        e.stopPropagation();

        if (e.key === "Escape") {
            setIsRecordingScreenshot(false);
            return;
        }

        const modifiers = [];
        if (e.ctrlKey) modifiers.push("Ctrl");
        if (e.shiftKey) modifiers.push("Shift");
        if (e.altKey) modifiers.push("Alt");
        if (e.metaKey) modifiers.push("Win");

        const key = e.key.toUpperCase();
        if (["CONTROL", "SHIFT", "ALT", "META"].includes(key)) return;

        const newHotkey = [...modifiers, key].join("+");
        setScreenshotHotkey(newHotkey);
        saveAppSetting("app.screenshot_hotkey", newHotkey);
        setIsRecordingScreenshot(false);
    }, [isRecordingScreenshot, saveAppSetting]);

    const handleQuickPasteHotkeyKeyDown = useCallback((e: React.KeyboardEvent) => {
        if (!isRecordingQuickPaste) return;
        e.preventDefault();
        e.stopPropagation();

        if (e.key === "Escape") {
            setIsRecordingQuickPaste(false);
            return;
        }

        const modifiers = [];
        if (e.ctrlKey) modifiers.push("Ctrl");
        if (e.shiftKey) modifiers.push("Shift");
        if (e.altKey) modifiers.push("Alt");
        if (e.metaKey) modifiers.push("Win");

        const key = e.key.toUpperCase();
        if (["CONTROL", "SHIFT", "ALT", "META"].includes(key)) return;

        const newHotkey = [...modifiers, key].join("+");
        setQuickPasteHotkey(newHotkey);
        saveAppSetting("app.quick_paste_hotkey", newHotkey);
        setIsRecordingQuickPaste(false);
    }, [isRecordingQuickPaste, saveAppSetting]);

    const handleCaptureNow = useCallback(() => {
        invoke("show_region_selector").catch((err) => {
            console.error("Screenshot failed:", err);
        });
    }, []);

    const screenshotHotkeyParts = screenshotHotkey ? screenshotHotkey.split("+") : [];
    const quickPasteHotkeyParts = quickPasteHotkey ? quickPasteHotkey.split("+") : [];

    return (
        <div className={`settings-group ${collapsed ? "collapsed" : ""}`}>
            <div className="group-header" onClick={onToggle}>
                <h3 style={{ margin: 0 }}>{t("capture_settings")}</h3>
                {collapsed ? <ChevronRight size={16} /> : <ChevronDown size={16} />}
            </div>
            {!collapsed && (
                <div className="group-content">
                    <div className="setting-item">
                        <LabelWithHint
                            label={t("screenshot_enabled")}
                            hint={t("screenshot_enabled_hint")}
                            hintKey="screenshot_enabled"
                        />
                        <label className="switch">
                            <input
                                className="cb"
                                type="checkbox"
                                checked={screenshotEnabled}
                                onChange={(e) => {
                                    const enabled = e.target.checked;
                                    setScreenshotEnabled(enabled);
                                    saveAppSetting("app.screenshot_enabled", String(enabled));
                                }}
                            />
                            <div className="toggle">
                                <div className="left" />
                                <div className="right" />
                            </div>
                        </label>
                    </div>

                    {screenshotEnabled && (
                        <>
                            <div className="setting-item" style={{ marginLeft: "18px" }}>
                                <div className="item-label-group">
                                    <span className="item-label">{t("screenshot_hotkey")}</span>
                                    <span className="hint">
                                        {isRecordingScreenshot ? (
                                            <div style={{ display: "flex", flexDirection: "column", gap: "2px" }}>
                                                <span style={{ color: "#ff9800", fontWeight: "bold" }}>
                                                    {t("win_key_not_recommended")}
                                                </span>
                                                <span style={{ fontSize: "11px", opacity: 0.8 }}>
                                                    {t("hotkey_recording_esc")}
                                                </span>
                                            </div>
                                        ) : t("hotkey_click_hint")}
                                    </span>
                                </div>
                                <div
                                    className={`key-group ${isRecordingScreenshot ? "recording" : ""}`}
                                    onClick={(e) => {
                                        setIsRecordingScreenshot(true);
                                        invoke("focus_clipboard_window").catch(console.error);
                                        e.currentTarget.focus();
                                    }}
                                    tabIndex={0}
                                    onKeyDown={handleScreenshotHotkeyKeyDown}
                                >
                                    {isRecordingScreenshot ? (
                                        <div className="key-cap" style={{ width: "8em" }}>
                                            {t("waiting_for_input")}
                                        </div>
                                    ) : screenshotHotkeyParts.length > 0 ? (
                                        screenshotHotkeyParts.map((k, i) => (
                                            <div key={i} className="key-cap">{k}</div>
                                        ))
                                    ) : (
                                        <div className="key-cap" style={{ width: "8em", opacity: 0.5 }}>
                                            {t("not_set")}
                                        </div>
                                    )}
                                </div>
                            </div>

                            <div className="setting-item" style={{ marginLeft: "18px" }}>
                                <div className="item-label-group">
                                    <span className="item-label">{t("capture_now")}</span>
                                </div>
                                <button
                                    className="setting-btn"
                                    onClick={handleCaptureNow}
                                >
                                    <Camera size={14} />
                                    {t("capture_now")}
                                </button>
                            </div>
                        </>
                    )}

                    <div className="setting-item">
                        <LabelWithHint
                            label={t("quick_paste_enabled")}
                            hint={t("quick_paste_enabled_hint")}
                            hintKey="quick_paste_enabled"
                        />
                        <label className="switch">
                            <input
                                className="cb"
                                type="checkbox"
                                checked={quickPasteEnabled}
                                onChange={(e) => {
                                    const enabled = e.target.checked;
                                    setQuickPasteEnabled(enabled);
                                    saveAppSetting("app.quick_paste_enabled", String(enabled));
                                }}
                            />
                            <div className="toggle">
                                <div className="left" />
                                <div className="right" />
                            </div>
                        </label>
                    </div>

                    {quickPasteEnabled && (
                        <div className="setting-item" style={{ marginLeft: "18px" }}>
                            <div className="item-label-group">
                                <span className="item-label">{t("quick_paste_hotkey_label")}</span>
                                <span className="hint">
                                    {isRecordingQuickPaste ? (
                                        <div style={{ display: "flex", flexDirection: "column", gap: "2px" }}>
                                            <span style={{ color: "#ff9800", fontWeight: "bold" }}>
                                                {t("win_key_not_recommended")}
                                            </span>
                                            <span style={{ fontSize: "11px", opacity: 0.8 }}>
                                                {t("hotkey_recording_esc")}
                                            </span>
                                        </div>
                                    ) : t("hotkey_click_hint")}
                                </span>
                            </div>
                            <div
                                className={`key-group ${isRecordingQuickPaste ? "recording" : ""}`}
                                onClick={(e) => {
                                    setIsRecordingQuickPaste(true);
                                    invoke("focus_clipboard_window").catch(console.error);
                                    e.currentTarget.focus();
                                }}
                                tabIndex={0}
                                onKeyDown={handleQuickPasteHotkeyKeyDown}
                            >
                                {isRecordingQuickPaste ? (
                                    <div className="key-cap" style={{ width: "8em" }}>
                                        {t("waiting_for_input")}
                                    </div>
                                ) : quickPasteHotkeyParts.length > 0 ? (
                                    quickPasteHotkeyParts.map((k, i) => (
                                        <div key={i} className="key-cap">{k}</div>
                                    ))
                                ) : (
                                    <div className="key-cap" style={{ width: "8em", opacity: 0.5 }}>
                                        {t("not_set")}
                                    </div>
                                )}
                            </div>
                        </div>
                    )}

                    <div className="setting-item">
                        <LabelWithHint
                            label={t("ocr_enabled")}
                            hint={t("ocr_enabled_hint")}
                            hintKey="ocr_enabled"
                        />
                        <label className="switch">
                            <input
                                className="cb"
                                type="checkbox"
                                checked={ocrEnabled}
                                onChange={(e) => {
                                    const enabled = e.target.checked;
                                    setOcrEnabled(enabled);
                                    saveAppSetting("app.ocr_enabled", String(enabled));
                                }}
                            />
                            <div className="toggle">
                                <div className="left" />
                                <div className="right" />
                            </div>
                        </label>
                    </div>

                    <div className="setting-item">
                        <div className="item-label-group">
                            <span className="item-label">{t("ocr_status_label")}</span>
                        </div>
                        {isWindows ? (
                            <span className="capture-engine-badge capture-engine-badge--available">
                                <span className="capture-engine-badge__dot" />
                                Windows Media.Ocr ({t("ocr_engine_available")})
                            </span>
                        ) : (
                            <span className="capture-engine-badge capture-engine-badge--unavailable">
                                <span className="capture-engine-badge__dot" />
                                Linux ({t("ocr_engine_unavailable")})
                            </span>
                        )}
                    </div>
                </div>
            )}
        </div>
    );
};

export default CaptureSettingsGroup;
