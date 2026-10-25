import { X } from "lucide-react";
import { useRef, useState } from "react";
import HotkeyRecorder from "./HotkeyRecorder";

export default function HotkeyListEditor({ t, labelKey, hotkeys, recording, setRecording, onAdd, onRemove, canRemove, disabled = false }: {
  readonly t: (key: string) => string;
  readonly labelKey: string;
  readonly hotkeys: readonly string[];
  readonly recording: boolean;
  readonly setRecording: (value: boolean) => void;
  readonly onAdd: (hotkey: string, options?: { readonly skipAvailabilityCheck?: boolean }) => Promise<boolean>;
  readonly onRemove: (hotkey: string) => Promise<boolean>;
  readonly canRemove?: (hotkey: string) => boolean;
  readonly disabled?: boolean;
}) {
  const pending = useRef(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const changeBinding = async (change: () => Promise<boolean>) => {
    if (disabled || pending.current) return;
    pending.current = true;
    setSaving(true);
    setError(null);
    try { await change(); }
    catch (failure) { setError(t("hotkey_register_failed") + String(failure)); }
    finally { pending.current = false; setSaving(false); }
  };
  return <div className="setting-item no-border column hotkey-list-editor">
    <HotkeyRecorder t={t} labelKey={labelKey} hotkey="" placeholder={`+ ${t(labelKey)}`}
      recording={recording} setRecording={setRecording} disabled={disabled || saving}
      onHotkeyChange={value => { void changeBinding(() => onAdd(value, { skipAvailabilityCheck: value === "MouseMiddle" })); }} />
    <div className="hotkey-list">
      {hotkeys.map((hotkey, index) => <div key={`${hotkey}-${index}`} className="hotkey-binding">
        <div className="key-group hotkey-binding-value">
          {hotkey.split("+").map((key, i) => <div key={`${key}-${i}`} className="key-cap">{key}</div>)}
        </div>
        {(!canRemove || canRemove(hotkey)) && <button type="button"
          className="btn-icon btn-icon-scalable btn-icon-size-hotkey" disabled={disabled || saving}
          onClick={() => { void changeBinding(() => onRemove(hotkey)); }} aria-label={`${t("delete")} ${hotkey}`} title={`${t("delete")} ${hotkey}`}>
          <X size={14} strokeWidth={2.6} />
        </button>}
      </div>)}
      {hotkeys.length === 0 && <div className="key-group hotkey-binding-value">
        <div className="key-cap key-cap--placeholder key-cap--unset">{t("not_set")}</div>
      </div>}
    </div>
    {error && <p className="hotkey-recording-error" role="alert">{error}</p>}
  </div>;
}
