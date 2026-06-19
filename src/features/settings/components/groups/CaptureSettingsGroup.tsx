import { type ComponentType, type ReactNode } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";

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
    screenshotHotkey,
    quickPasteEnabled,
    setQuickPasteEnabled,
    quickPasteHotkey,
    ocrEnabled,
    setOcrEnabled,
    saveAppSetting,
}: CaptureSettingsGroupProps) => {
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
                        <div className="setting-item" style={{ marginLeft: "18px" }}>
                            <div className="item-label-group">
                                <span className="item-label">{t("screenshot_hotkey")}</span>
                            </div>
                            <div
                                className="hotkey-display"
                                style={{ cursor: "pointer" }}
                            >
                                <span>{screenshotHotkey || t("not_set")}</span>
                            </div>
                        </div>
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
                            </div>
                            <span>{quickPasteHotkey || t("not_set")}</span>
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
                        <span style={{ fontSize: "12px", opacity: 0.7 }}>
                            {navigator.platform.startsWith("Win")
                                ? t("ocr_engine_windows")
                                : t("ocr_engine_linux")}
                        </span>
                    </div>
                </div>
            )}
        </div>
    );
};

export default CaptureSettingsGroup;
