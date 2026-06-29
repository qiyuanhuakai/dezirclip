import { memo, useState, useEffect } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { HelpCircle } from "lucide-react";
import { motion } from "framer-motion";
import type { LabelWithHintProps, SettingsPanelProps } from "./SettingsPanel.types";
import SettingsGroups from "./SettingsGroups";
import SettingsFooter from "./SettingsFooter";

const SettingsPanel = (props: SettingsPanelProps) => {
    const { t, appSettings, handleResetSettings } = props;

    const [appVersion, setAppVersion] = useState("");
    const [openHints, setOpenHints] = useState<Set<string>>(new Set());
    const [privacyKindsOpen, setPrivacyKindsOpen] = useState(false);
    const [privacyRulesOpen, setPrivacyRulesOpen] = useState(false);

    const [screenshotEnabled, setScreenshotEnabled] = useState(() => {
        const val = appSettings?.["app.screenshot_enabled"];
        return val !== "false";
    });
    const [screenshotHotkey] = useState(() => {
        return appSettings?.["app.screenshot_hotkey"] || "Ctrl+Shift+A";
    });
    const [quickPasteEnabled, setQuickPasteEnabled] = useState(() => {
        const val = appSettings?.["app.quick_paste_enabled"];
        return val !== "false";
    });
    const [ocrEnabled, setOcrEnabled] = useState(() => {
        const val = appSettings?.["app.ocr_enabled"];
        return val !== "false";
    });

    const toggleHint = (key: string) => {
        setOpenHints(prev => {
            const next = new Set(prev);
            if (next.has(key)) next.delete(key);
            else next.add(key);
            return next;
        });
    };

    const LabelWithHint = ({ label, hint, hintKey, labelStyle }: LabelWithHintProps) => (
        <div className="item-label-group">
            <div style={{ display: "flex", alignItems: "center", gap: "6px" }}>
                <span className="item-label" style={labelStyle}>{label}</span>
                {hint && (
                    <button
                        type="button"
                        className="hint-icon-btn"
                        title={typeof hint === "string" ? hint : undefined}
                        onClick={(e) => {
                            e.stopPropagation();
                            toggleHint(hintKey);
                        }}
                    >
                        <HelpCircle size={12} />
                    </button>
                )}
            </div>
            {hint && openHints.has(hintKey) && (
                typeof hint === "string" ? <span className="hint">{hint}</span> : hint
            )}
        </div>
    );

    useEffect(() => {
        getVersion()
            .then(v => setAppVersion(v))
            .catch(err => {
                console.error("Failed to get version:", err);
                setAppVersion("0.2.0");
            });
    }, []);

    return (
        <motion.div
            initial={{ opacity: 0, x: 20 }}
            animate={{ opacity: 1, x: 0 }}
            style={{ display: "flex", flexDirection: "column", gap: "4px" }}
        >
            <SettingsGroups
                settings={props}
                LabelWithHint={LabelWithHint}
                privacyKindsOpen={privacyKindsOpen}
                setPrivacyKindsOpen={setPrivacyKindsOpen}
                privacyRulesOpen={privacyRulesOpen}
                setPrivacyRulesOpen={setPrivacyRulesOpen}
                screenshotEnabled={screenshotEnabled}
                setScreenshotEnabled={setScreenshotEnabled}
                screenshotHotkey={screenshotHotkey}
                quickPasteEnabled={quickPasteEnabled}
                setQuickPasteEnabled={setQuickPasteEnabled}
                ocrEnabled={ocrEnabled}
                setOcrEnabled={setOcrEnabled}
            />

            <SettingsFooter
                t={t}
                appVersion={appVersion}
                onResetSettings={handleResetSettings}
            />

        </motion.div>
    );
};

export default memo(SettingsPanel);
