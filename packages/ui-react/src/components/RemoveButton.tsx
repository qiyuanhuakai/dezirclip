import type { ComponentPropsWithRef } from "react";
import { Minus, Trash2 } from "lucide-react";

interface RemoveButtonProps extends ComponentPropsWithRef<"button"> {
  label: string;
}

export function HotkeyRemoveButton({ label, title = label, className = "", ...props }: RemoveButtonProps) {
  return <button type="button" className={["btn-icon btn-icon-scalable btn-icon-size-hotkey", className].filter(Boolean).join(" ")}
    aria-label={label} title={title} {...props}><Minus size={14} strokeWidth={2.6} /></button>;
}

export function SettingsRemoveButton({ label, title = label, className = "", iconOnly, ...props }: RemoveButtonProps & {
  iconOnly?: boolean;
}) {
  return <button type="button" aria-label={label} title={title}
    className={["btn-icon settings-action settings-remove", iconOnly && "settings-remove-icon", className].filter(Boolean).join(" ")}
    {...props}>
    <Trash2 size={14} aria-hidden="true" />{!iconOnly && <span>{label}</span>}
  </button>;
}
