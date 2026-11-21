import type { ComponentProps, ReactNode } from "react";
import { SettingHelp } from "./SettingHelp";
import { SettingsField } from "./SettingsField";

export type SettingsToggleProps = Omit<ComponentProps<"input">, "className" | "type">;

export function SettingsToggle(props: SettingsToggleProps) {
  return <label className="switch">
    <input {...props} className="cb" type="checkbox" />
    <div className="toggle"><div className="left" /><div className="right" /></div>
  </label>;
}

export function SettingsToggleField({ label, helpLabel, description, ...toggle }: SettingsToggleProps & {
  label: string;
  helpLabel: string;
  description: ReactNode;
}) {
  return <SettingsField>
    <div className="item-label-group">
      <div className="settings-label-line">
        <span className="item-label">{label}</span>
        {description && <SettingHelp label={label} helpLabel={helpLabel}>{description}</SettingHelp>}
      </div>
    </div>
    <SettingsToggle {...toggle} aria-label={label} />
  </SettingsField>;
}
