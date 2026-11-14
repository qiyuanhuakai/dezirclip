import { createContext, useContext, useId, useState, type ReactNode } from "react";
import { CircleHelp } from "lucide-react";
import type { LabelWithHintProps } from "./SettingsPanel.types";

export const SettingsTranslation = createContext<(key: string) => string>(key => key);

export function SettingHelp({ children, label }: { readonly children: ReactNode; readonly label?: string }) {
    const t = useContext(SettingsTranslation);
    const [open, setOpen] = useState(false);
    const id = useId();
    return <span className="setting-help">
        <button type="button" className="setting-help-button" aria-label={`${t("settings_help")}${label ? `: ${label}` : ""}`}
            aria-expanded={open} aria-controls={id} onClick={() => setOpen(value => !value)}>
            <CircleHelp size={15} aria-hidden="true" />
        </button>
        <span id={id} className="setting-help-text" hidden={!open}>{children}</span>
    </span>;
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
