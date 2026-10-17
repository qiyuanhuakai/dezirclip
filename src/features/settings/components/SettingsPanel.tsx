import { memo, useState, useEffect } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { HelpCircle } from "lucide-react";
import { motion } from "framer-motion";
import type { LabelWithHintProps, SettingsPanelProps } from "./SettingsPanel.types";
import SettingsGroups from "./SettingsGroups";
import SettingsRail from "./SettingsRail";
import SettingsFooter from "./SettingsFooter";
import { useSettingsLayout } from "../hooks/useSettingsLayout";
import { DEFAULT_SETTINGS_GROUP, type SettingsGroupId } from "../utils/settingsLayout";

const SettingsPanel = (props: SettingsPanelProps) => {
    const { t, appSettings, handleResetSettings } = props;

    const layout = useSettingsLayout();
    const [activeGroup, setActiveGroup] = useState<SettingsGroupId>(DEFAULT_SETTINGS_GROUP);

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

    const groups = (
        <SettingsGroups
            settings={props}
            LabelWithHint={LabelWithHint}
            split={layout.split}
            activeGroup={activeGroup}
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
    );

    // Stack keeps the centred single column the accordion has always had. Split
    // gives the rail and the pane a bounded height so the pane is what scrolls,
    // and folds the footer onto one line so it costs a row rather than a screen.
    const paneClass = `settings-pane settings-pane--cols-${layout.paneColumns}`;

    return (
        <motion.div
            initial={{ opacity: 0, x: 20 }}
            animate={{ opacity: 1, x: 0 }}
            className={`settings-layout settings-layout--${layout.split ? "split" : "stack"}`}
        >
            {layout.split ? (
                <div className="settings-split-row">
                    <SettingsRail t={t} activeGroup={activeGroup} onSelect={setActiveGroup} />
                    <div className={paneClass}>{groups}</div>
                </div>
            ) : (
                groups
            )}

            <SettingsFooter
                t={t}
                appVersion={appVersion}
                onResetSettings={handleResetSettings}
                compact={layout.split}
            />

        </motion.div>
    );
};

export default memo(SettingsPanel);
