import type { ComponentType, ReactNode } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import ToolsCaptureSection from "./ToolsCaptureSection";
import ToolsCliSection from "./ToolsCliSection";

interface LabelWithHintProps {
    readonly label: string;
    readonly hint?: string | ReactNode;
    readonly hintKey: string;
}

interface ToolsSettingsGroupProps {
    readonly t: (key: string) => string;
    readonly collapsed: boolean;
    readonly onToggle: () => void;
    readonly LabelWithHint: ComponentType<LabelWithHintProps>;
    readonly screenshotEnabled: boolean;
    readonly setScreenshotEnabled: (val: boolean) => void;
    readonly screenshotHotkey: string;
    readonly quickPasteEnabled: boolean;
    readonly setQuickPasteEnabled: (val: boolean) => void;
    readonly quickPasteHotkey: string;
    readonly ocrEnabled: boolean;
    readonly setOcrEnabled: (val: boolean) => void;
    readonly saveAppSetting: (key: string, val: string) => void;
}

const ToolsSettingsGroup = ({
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
}: ToolsSettingsGroupProps) => (
    <div className={`settings-group ${collapsed ? "collapsed" : ""}`}>
        <div className="group-header" onClick={onToggle}>
            <h3 style={{ margin: 0 }}>{t("tools_settings")}</h3>
            {collapsed ? <ChevronRight size={16} /> : <ChevronDown size={16} />}
        </div>
        {!collapsed && (
            <div className="group-content">
                <ToolsCaptureSection
                    t={t}
                    LabelWithHint={LabelWithHint}
                    screenshotEnabled={screenshotEnabled}
                    setScreenshotEnabled={setScreenshotEnabled}
                    screenshotHotkey={screenshotHotkey}
                    quickPasteEnabled={quickPasteEnabled}
                    setQuickPasteEnabled={setQuickPasteEnabled}
                    quickPasteHotkey={quickPasteHotkey}
                    ocrEnabled={ocrEnabled}
                    setOcrEnabled={setOcrEnabled}
                    saveAppSetting={saveAppSetting}
                />
                <ToolsCliSection t={t} LabelWithHint={LabelWithHint} />
            </div>
        )}
    </div>
);

export default ToolsSettingsGroup;
