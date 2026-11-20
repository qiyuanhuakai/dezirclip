import type { ComponentProps } from "react";

export type SettingsToggleProps = Omit<ComponentProps<"input">, "className" | "type">;

export function SettingsToggle(props: SettingsToggleProps) {
  return <label className="switch">
    <input {...props} className="cb" type="checkbox" />
    <div className="toggle"><div className="left" /><div className="right" /></div>
  </label>;
}
