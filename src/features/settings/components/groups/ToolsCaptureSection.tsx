import { useCallback, useEffect, useState, type ComponentType, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Camera } from "lucide-react";

interface LabelWithHintProps {
    readonly label: string;
    readonly hint?: string | ReactNode;
    readonly hintKey: string;
}

interface ToolsCaptureSectionProps {
    readonly t: (key: string) => string;
    readonly LabelWithHint: ComponentType<LabelWithHintProps>;
    readonly screenshotEnabled: boolean;
    readonly setScreenshotEnabled: (val: boolean) => void;
    readonly screenshotHotkey: string;
    readonly screenshotFullHotkey: string;
    readonly quickPasteEnabled: boolean;
    readonly setQuickPasteEnabled: (val: boolean) => void;
    readonly quickPasteHotkey: string;
    readonly ocrEnabled: boolean;
    readonly setOcrEnabled: (val: boolean) => void;
    readonly saveAppSetting: (key: string, val: string) => void;
}

interface HotkeyRecorderProps {
    readonly t: (key: string) => string;
    readonly labelKey: string;
    readonly hotkey: string;
    readonly recording: boolean;
    readonly setRecording: (val: boolean) => void;
    readonly onHotkeyChange: (val: string) => void;
}

const buildHotkey = (e: React.KeyboardEvent): string | null => {
    const key = e.key.toUpperCase();
    if (["CONTROL", "SHIFT", "ALT", "META"].includes(key)) return null;
    const modifiers = [];
    if (e.ctrlKey) modifiers.push("Ctrl");
    if (e.shiftKey) modifiers.push("Shift");
    if (e.altKey) modifiers.push("Alt");
    if (e.metaKey) modifiers.push("Win");
    return [...modifiers, key].join("+");
};

const HotkeyRecorder = ({ t, labelKey, hotkey, recording, setRecording, onHotkeyChange }: HotkeyRecorderProps) => {
    const parts = hotkey ? hotkey.split("+") : [];
    const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
        if (!recording) return;
        e.preventDefault();
        e.stopPropagation();
        if (e.key === "Escape") {
            setRecording(false);
            return;
        }
        const newHotkey = buildHotkey(e);
        if (!newHotkey) return;
        onHotkeyChange(newHotkey);
        setRecording(false);
    }, [onHotkeyChange, recording, setRecording]);

    return (
        <div className="setting-item setting-item--detail">
            <div className="item-label-group">
                <span className="item-label">{t(labelKey)}</span>
                <span className="hint">
                    {recording ? (
                        <div style={{ display: "flex", flexDirection: "column", gap: "2px" }}>
                            <span style={{ color: "#ff9800", fontWeight: "bold" }}>{t("win_key_not_recommended")}</span>
                            <span style={{ fontSize: "11px", opacity: 0.8 }}>{t("hotkey_recording_esc")}</span>
                        </div>
                    ) : t("hotkey_click_hint")}
                </span>
            </div>
            <div
                className={`key-group ${recording ? "recording" : ""}`}
                onClick={(e) => {
                    setRecording(true);
                    invoke("focus_clipboard_window").catch(console.error);
                    e.currentTarget.focus();
                }}
                tabIndex={0}
                onKeyDown={handleKeyDown}
            >
                {recording ? (
                    <div className="key-cap" style={{ width: "8em" }}>{t("waiting_for_input")}</div>
                ) : parts.length > 0 ? (
                    parts.map((part, index) => <div key={`${part}-${index}`} className="key-cap">{part}</div>)
                ) : (
                    <div className="key-cap" style={{ width: "8em", opacity: 0.5 }}>{t("not_set")}</div>
                )}
            </div>
        </div>
    );
};

interface OcrEngineInfo {
    readonly available: boolean;
    readonly engine_name: string;
}

// `check_ocr_engine_available` builds an `OcrService`, and the Linux one runs
// `tesseract --version` and waits for it to exit -- a subprocess per call, with no
// shared cache on the Rust side. Whether an engine is installed cannot change
// while the app runs, but this section is folded away by any resize that drops
// the window back to the narrow layout, which remounted it and probed again.
// Holding the promise at module scope matches the font scan and the CLI lookup
// next door: callers arriving while a probe is in flight share it, later mounts
// reuse the answer, and a rejection clears it so a later visit can retry.
//
// Caching alone is not enough on its own. The backend answers "no engine" as a
// fulfilled `available=false`, not a rejection, so a cached `false` was permanent
// for the life of the webview: installing tesseract mid-session left the badge
// claiming it was missing until the window was rebuilt. `forceRefresh` drops the
// held promise so the row's own retry can ask again, and nothing else calls it --
// no resize and no remount does.
let ocrEngineRequest: Promise<OcrEngineInfo> | null = null;
const loadOcrEngineInfo = (forceRefresh = false): Promise<OcrEngineInfo> => {
    if (forceRefresh) ocrEngineRequest = null;
    ocrEngineRequest ??= invoke<OcrEngineInfo>("check_ocr_engine_available").catch((err: unknown) => {
        ocrEngineRequest = null;
        throw err;
    });
    return ocrEngineRequest;
};

const ToolsCaptureSection = ({
    t,
    LabelWithHint,
    screenshotEnabled,
    setScreenshotEnabled,
    screenshotHotkey: initialScreenshotHotkey,
    screenshotFullHotkey: initialScreenshotFullHotkey,
    quickPasteEnabled,
    setQuickPasteEnabled,
    quickPasteHotkey: initialQuickPasteHotkey,
    ocrEnabled,
    setOcrEnabled,
    saveAppSetting,
}: ToolsCaptureSectionProps) => {
    const [screenshotHotkey, setScreenshotHotkey] = useState(initialScreenshotHotkey);
    const [screenshotFullHotkey, setScreenshotFullHotkey] = useState(initialScreenshotFullHotkey);
    const [quickPasteHotkey, setQuickPasteHotkey] = useState(initialQuickPasteHotkey);
    const [isRecordingScreenshot, setIsRecordingScreenshot] = useState(false);
    const [isRecordingScreenshotFull, setIsRecordingScreenshotFull] = useState(false);
    const [isRecordingQuickPaste, setIsRecordingQuickPaste] = useState(false);
    const [ocrEngineInfo, setOcrEngineInfo] = useState<OcrEngineInfo | null>(null);
    const [ocrLoading, setOcrLoading] = useState(false);

    const refreshOcrEngineInfo = useCallback((forceRefresh = false) => {
        setOcrLoading(true);
        loadOcrEngineInfo(forceRefresh)
            .then(setOcrEngineInfo)
            .catch((err) => {
                console.error("Failed to query OCR engine availability:", err);
            })
            .finally(() => setOcrLoading(false));
    }, []);

    useEffect(() => {
        refreshOcrEngineInfo();
    }, [refreshOcrEngineInfo]);
    useEffect(() => setScreenshotHotkey(initialScreenshotHotkey), [initialScreenshotHotkey]);
    useEffect(() => setScreenshotFullHotkey(initialScreenshotFullHotkey), [initialScreenshotFullHotkey]);
    useEffect(() => setQuickPasteHotkey(initialQuickPasteHotkey), [initialQuickPasteHotkey]);

    const updateScreenshotHotkey = useCallback((newHotkey: string) => {
        setScreenshotHotkey(newHotkey);
        saveAppSetting("screenshot_hotkey", newHotkey);
    }, [saveAppSetting]);

    const updateScreenshotFullHotkey = useCallback((newHotkey: string) => {
        setScreenshotFullHotkey(newHotkey);
        saveAppSetting("screenshot_full_hotkey", newHotkey);
    }, [saveAppSetting]);

    const updateQuickPasteHotkey = useCallback((newHotkey: string) => {
        setQuickPasteHotkey(newHotkey);
        saveAppSetting("quick_paste_hotkey", newHotkey);
    }, [saveAppSetting]);

    return (
        <>
            <div className="setting-item"><div className="item-label-group"><span className="item-label">{t("capture_settings")}</span></div></div>
            <div className="setting-item">
                <LabelWithHint label={t("screenshot_enabled")} hint={t("screenshot_enabled_hint")} hintKey="screenshot_enabled" />
                <label className="switch"><input className="cb" type="checkbox" checked={screenshotEnabled} onChange={(e) => { setScreenshotEnabled(e.target.checked); saveAppSetting("screenshot_enabled", String(e.target.checked)); }} /><div className="toggle"><div className="left" /><div className="right" /></div></label>
            </div>
            {screenshotEnabled && (
                <>
                    <HotkeyRecorder t={t} labelKey="screenshot_hotkey" hotkey={screenshotHotkey} recording={isRecordingScreenshot} setRecording={setIsRecordingScreenshot} onHotkeyChange={updateScreenshotHotkey} />
                    <HotkeyRecorder t={t} labelKey="screenshot_full_hotkey" hotkey={screenshotFullHotkey} recording={isRecordingScreenshotFull} setRecording={setIsRecordingScreenshotFull} onHotkeyChange={updateScreenshotFullHotkey} />
                    <div className="setting-item setting-item--detail">
                        <div className="item-label-group"><span className="item-label">{t("capture_now")}</span></div>
                        <div className="setting-btn-group">
                            <button className="setting-btn" onClick={() => invoke("show_region_selector").catch(console.error)}><Camera size={14} />{t("capture_region_now")}</button>
                            <button className="setting-btn" onClick={() => invoke("capture_full_screen").catch(console.error)}><Camera size={14} />{t("capture_full_now")}</button>
                        </div>
                    </div>
                </>
            )}
            <div className="setting-item">
                <LabelWithHint label={t("quick_paste_enabled")} hint={t("quick_paste_enabled_hint")} hintKey="quick_paste_enabled" />
                <label className="switch"><input className="cb" type="checkbox" checked={quickPasteEnabled} onChange={(e) => { setQuickPasteEnabled(e.target.checked); saveAppSetting("quick_paste_enabled", String(e.target.checked)); }} /><div className="toggle"><div className="left" /><div className="right" /></div></label>
            </div>
            {quickPasteEnabled && <HotkeyRecorder t={t} labelKey="quick_paste_hotkey_label" hotkey={quickPasteHotkey} recording={isRecordingQuickPaste} setRecording={setIsRecordingQuickPaste} onHotkeyChange={updateQuickPasteHotkey} />}
            <div className="setting-item">
                <LabelWithHint label={t("ocr_enabled")} hint={t("ocr_enabled_hint")} hintKey="ocr_enabled" />
                <label className="switch"><input className="cb" type="checkbox" checked={ocrEnabled} onChange={(e) => { setOcrEnabled(e.target.checked); saveAppSetting("ocr_enabled", String(e.target.checked)); }} /><div className="toggle"><div className="left" /><div className="right" /></div></label>
            </div>
            <div className="setting-item">
                <div className="item-label-group"><span className="item-label">{t("ocr_status_label")}</span></div>
                <div className="tools-engine-status-actions">
                    <span className={`capture-engine-badge capture-engine-badge--${ocrEngineInfo?.available ? "available" : "unavailable"}`}>
                        <span className="capture-engine-badge__dot" />
                        {ocrEngineInfo
                            ? `${ocrEngineInfo.engine_name} (${t(ocrEngineInfo.available ? "ocr_engine_available" : "ocr_engine_unavailable")})`
                            : t("common_loading")}
                    </span>
                    <button
                        type="button"
                        className="setting-btn setting-btn--compact"
                        onClick={() => refreshOcrEngineInfo(true)}
                        disabled={ocrLoading}
                    >
                        {ocrLoading ? t("processing") : t("common_retry")}
                    </button>
                </div>
            </div>
        </>
    );
};

export default ToolsCaptureSection;
