import type { ComponentProps, ReactNode } from "react";
import { HotkeyRemoveButton } from "./RemoveButton";

function hotkeyKeys(value: string, platform = globalThis.navigator?.platform ?? ""): string[] {
  const modifier = /^win/i.test(platform) ? "Win" : /^linux/i.test(platform) ? "Super" : undefined;
  const keys = value.split("+");
  if (!modifier) return keys;
  return keys.map((key, index) => index < keys.length - 1 && /^(super|meta|win|windows|cmd|command)$/i.test(key.trim())
    ? modifier : key);
}

export interface HotkeyBindingProps {
  platform?: string;
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

export interface HotkeyListProps {
  platform?: string;
  values: readonly string[];
  emptyLabel: string;
  removeLabel: string;
  disabled?: boolean;
  canRemove?: (value: string) => boolean;
  onRemove?: (value: string) => void;
}

export function HotkeyListLayout({ children }: { children: ReactNode }) {
  return <div className="setting-item no-border column hotkey-list-editor">{children}</div>;
}

export function HotkeyList({ platform, values, emptyLabel, removeLabel, disabled, canRemove, onRemove }: HotkeyListProps) {
  return <div className="hotkey-list">
    {values.map((value, index) => {
      const keys = hotkeyKeys(value, platform);
      return <div key={value + index} className="hotkey-binding">
        <div className="key-group hotkey-binding-value">
          {keys.map((key, position) => <div key={key + position} className="key-cap">{key}</div>)}
        </div>
        {values.length > 1 && onRemove && (!canRemove || canRemove(value)) &&
          <HotkeyRemoveButton disabled={disabled} onClick={() => onRemove(value)} label={removeLabel + " " + keys.join("+")} />}
      </div>;
    })}
    {values.length === 0 && <div className="key-group hotkey-binding-value">
      <div className="key-cap key-cap--placeholder key-cap--unset">{emptyLabel}</div>
    </div>}
  </div>;
}

export function HotkeyBinding(props: HotkeyBindingProps) {
  const active = props.recording && !props.disabled;
  const keys = props.value ? hotkeyKeys(props.value, props.platform) : [];
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
      {props.onClear && props.value && !props.recording && <HotkeyRemoveButton disabled={props.disabled}
        onClick={props.onClear} label={props.clearLabel ?? props.label} title={props.clearTitle} />}
    </div>
    {props.error && <p className="hotkey-recording-error" role="alert">{props.error}</p>}
  </div>;
}
