import { memo, useState } from "react";
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

    const LabelWithHint = ({ label, labelStyle }: LabelWithHintProps) => (
        <div className="item-label-group">
            <span className="item-label" style={labelStyle}>{label}</span>
        </div>
    );

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
            {/* This wrapper exists only to keep `groups` at the same position and
                the same element type across the breakpoint. Rendering `groups`
                directly in stack mode and a `<div>` around it in split mode made
                React rebuild every group on each crossing, dropping open dialogs
                and half-filled forms. `display: contents` lifts the wrapper out of
                the flow in stack mode, so the accordion renders exactly as before;
                the rail is present but hidden, which keeps the child list identical
                on both sides rather than relying on how React reconciles a null
                slot. */}
            <div className={`settings-split-row${layout.split ? "" : " settings-split-row--inert"}`}>
                <SettingsRail t={t} activeGroup={activeGroup} onSelect={setActiveGroup} />
                <div className={layout.split ? paneClass : "settings-pane settings-pane--inert"}>{groups}</div>
            </div>

            <SettingsFooter
                t={t}
                onResetSettings={handleResetSettings}
                compact={layout.split}
            />

        </motion.div>
    );
};

export default memo(SettingsPanel);
