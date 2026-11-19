import { createContext, useContext, type ReactNode } from "react";
import { SettingHelp as Help } from "@deziro/ui-react/components/SettingHelp";
import type { LabelWithHintProps } from "./SettingsPanel.types";

export const SettingsTranslation = createContext<(key: string) => string>(key => key);

export function SettingHelp({ children, label }: { readonly children: ReactNode; readonly label?: string }) {
    const t = useContext(SettingsTranslation);
    return <Help label={label} helpLabel={t("settings_help")}>{children}</Help>;
}

export function SettingsLabel({ label, hint, hintKey, labelStyle }: LabelWithHintProps) {
    const t = useContext(SettingsTranslation);
    const key = `${hintKey}_hint`;
    const translated = t(key);
    const description = hint ?? (translated !== key ? translated : null);
    return <div className="item-label-group">
        <div className="settings-label-line">
            <span className="item-label" style={labelStyle}>{label}</span>
            {description && <SettingHelp label={label}>{description}</SettingHelp>}
        </div>
    </div>;
}
