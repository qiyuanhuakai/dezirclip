import { SettingsToggle } from "@deziro/ui-react/components/SettingsToggle";
import { CaptureEngineStatus, CaptureSettingsActions, CaptureSettingsHeading } from "@deziro/ui-react/components/SettingsCapture";
import NativeFeatureBoundary from "../NativeFeatureBoundary";
import { useCallback, useEffect, useState, type ComponentType, type ReactNode } from "react";
import { invoke } from "../../../../shared/lib/desktopInvoke";
import HotkeyRecorder from "../HotkeyRecorder";
import QuickPasteRendererSetting from "./QuickPasteRendererSetting";

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
    readonly quickPasteRenderer: string;
    readonly ocrEnabled: boolean;
    readonly setOcrEnabled: (val: boolean) => void;
    readonly saveAppSetting: (key: string, val: string) => void | Promise<boolean | void>;
}

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
    quickPasteRenderer,
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
    const [ocrLoadFailed, setOcrLoadFailed] = useState(false);
    const [panelSaving, setPanelSaving] = useState(false);
    const [hotkeyError, setHotkeyError] = useState<string | null>(null);
    const [captureError, setCaptureError] = useState<string | null>(null);
    const [capturing, setCapturing] = useState(false);
    const captureNow = async (command: "show_region_selector" | "capture_full_screen") => {
        setCapturing(true);
        setCaptureError(null);
        try {
            await invoke(command);
        } catch (error) {
            setCaptureError(`${t("screenshot_failed")}: ${String(error)}`);
        } finally {
            setCapturing(false);
        }
    };
    const savePanelEnabled = async (key: string, enabled: boolean, apply: (value: boolean) => void) => {
        setPanelSaving(true);
        try {
            if (await saveAppSetting(key, String(enabled)) !== false) apply(enabled);
        } finally { setPanelSaving(false); }
    };

    const refreshOcrEngineInfo = useCallback((forceRefresh = false) => {
        setOcrLoading(true);
        setOcrLoadFailed(false);
        loadOcrEngineInfo(forceRefresh)
            .then(setOcrEngineInfo)
            .catch((err) => {
                setOcrLoadFailed(true);
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

    const savePanelHotkey = useCallback(async (key: string, value: string, apply: (value: string) => void) => {
        setPanelSaving(true);
        setHotkeyError(null);
        try {
            if (await saveAppSetting(key, value) !== false) apply(value);
            else setHotkeyError(t("hotkey_unavailable"));
        } finally { setPanelSaving(false); }
    }, [saveAppSetting, t]);

    return (
        <>
            <CaptureSettingsHeading label={t("capture_settings")} />
            <NativeFeatureBoundary t={t}>
            <div className="setting-item">
                <LabelWithHint label={t("screenshot_enabled")} hintKey="screenshot_enabled" />
                <SettingsToggle aria-label={t("screenshot_enabled")} checked={screenshotEnabled} disabled={panelSaving} onChange={(e) => { void savePanelEnabled("screenshot_enabled", e.target.checked, setScreenshotEnabled); }} />
            </div>
            {screenshotEnabled && (
                <>
                    <HotkeyRecorder t={t} labelKey="screenshot_hotkey" hotkey={screenshotHotkey} recording={isRecordingScreenshot} setRecording={setIsRecordingScreenshot} disabled={panelSaving} onHotkeyChange={value => { void savePanelHotkey("screenshot_hotkey", value, setScreenshotHotkey); }} />
                    <HotkeyRecorder t={t} labelKey="screenshot_full_hotkey" hotkey={screenshotFullHotkey} recording={isRecordingScreenshotFull} setRecording={setIsRecordingScreenshotFull} disabled={panelSaving} onHotkeyChange={value => { void savePanelHotkey("screenshot_full_hotkey", value, setScreenshotFullHotkey); }} />
                    <CaptureSettingsActions label={t("capture_now")} region={t("capture_region_now")} full={t("capture_full_now")}
                        disabled={capturing} error={captureError} onRegion={() => void captureNow("show_region_selector")}
                        onFull={() => void captureNow("capture_full_screen")} />
                </>
            )}
            <div className="setting-item">
                <LabelWithHint label={t("quick_paste_enabled")} hintKey="quick_paste_enabled" />
                <SettingsToggle aria-label={t("quick_paste_enabled")} checked={quickPasteEnabled} disabled={panelSaving} onChange={(e) => { void savePanelEnabled("quick_paste_enabled", e.target.checked, setQuickPasteEnabled); }} />
            </div>
            {quickPasteEnabled && <HotkeyRecorder t={t} labelKey="quick_paste_hotkey_label" hotkey={quickPasteHotkey} recording={isRecordingQuickPaste} setRecording={setIsRecordingQuickPaste} disabled={panelSaving} onHotkeyChange={value => { void savePanelHotkey("quick_paste_hotkey", value, setQuickPasteHotkey); }} />}
            {hotkeyError && <p role="alert" className="hotkey-recording-error">{hotkeyError}</p>}
            {quickPasteEnabled && <QuickPasteRendererSetting t={t} renderer={quickPasteRenderer} />}
            </NativeFeatureBoundary>
            <div className="setting-item">
                <LabelWithHint label={t("ocr_enabled")} hintKey="ocr_enabled" />
                <SettingsToggle checked={ocrEnabled} onChange={(e) => { setOcrEnabled(e.target.checked); saveAppSetting("ocr_enabled", String(e.target.checked)); }} />
            </div>
            <CaptureEngineStatus label={t("ocr_status_label")} available={!ocrLoadFailed && Boolean(ocrEngineInfo?.available)}
                status={ocrLoadFailed ? t("ocr_engine_check_failed") : ocrEngineInfo
                    ? `${ocrEngineInfo.engine_name} (${t(ocrEngineInfo.available ? "ocr_engine_available" : "ocr_engine_unavailable")})`
                    : t("common_loading")}
                retry={ocrLoading ? t("processing") : t("common_retry")} disabled={ocrLoading} onRetry={() => refreshOcrEngineInfo(true)} />
        </>
    );
};

export default ToolsCaptureSection;
