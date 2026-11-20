import type { ComponentProps } from "react";
import { X } from "lucide-react";

export interface HotkeyBindingProps {
  label: string;
  value: string;
  placeholder: string;
  unset?: boolean;
  recordingLabel: string;
  recording: boolean;
  busy: boolean;
  disabled: boolean;
  clearLabel?: string;
  clearTitle?: string;
  error?: string | null;
  onClear?: () => void;
  onClick: ComponentProps<"div">["onClick"];
  onKeyDown: ComponentProps<"div">["onKeyDown"];
  onMouseDown: ComponentProps<"div">["onMouseDown"];
  onBlur: ComponentProps<"div">["onBlur"];
}

export function HotkeyBinding(props: HotkeyBindingProps) {
  const active = props.recording && !props.disabled;
  const keys = props.value ? props.value.split("+") : [];
  return <div className="setting-item setting-item--detail hotkey-recorder">
    <div className="item-label-group"><span className="item-label">{props.label}</span></div>
    <div className="hotkey-binding">
      <div className={`key-group ${active ? "recording" : ""}`} role="button" aria-label={props.label}
        aria-pressed={active} aria-busy={props.busy} aria-disabled={props.disabled} tabIndex={props.disabled ? -1 : 0}
        onClick={props.onClick} onKeyDown={props.onKeyDown} onMouseDown={props.onMouseDown} onBlur={props.onBlur}>
        {active || keys.length === 0
          ? <div className={`key-cap key-cap--placeholder${!active && props.unset ? " key-cap--unset" : ""}`}>
            {active ? props.recordingLabel : props.placeholder}
          </div>
          : keys.map((key, index) => <div key={`${key}-${index}`} className="key-cap">{key}</div>)}
      </div>
      {props.onClear && props.value && !props.recording && <button type="button"
        className="btn-icon btn-icon-scalable btn-icon-size-hotkey" disabled={props.disabled}
        onClick={props.onClear} aria-label={props.clearLabel} title={props.clearTitle}>
        <X size={14} strokeWidth={2.6} />
      </button>}
    </div>
    {props.error && <p className="hotkey-recording-error" role="alert">{props.error}</p>}
  </div>;
}
