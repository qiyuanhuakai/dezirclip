import { HotkeyList } from "@deziro/ui-react/components/HotkeyBinding";
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
    try { if (!await change()) setError(t("hotkey_unavailable")); }
    catch (failure) { setError(t("hotkey_register_failed") + String(failure)); }
    finally { pending.current = false; setSaving(false); }
  };
  return <div className="setting-item no-border column hotkey-list-editor">
    <HotkeyRecorder t={t} labelKey={labelKey} hotkey="" placeholder={`+ ${t(labelKey)}`}
      recording={recording} setRecording={setRecording} disabled={disabled || saving}
      onHotkeyChange={value => { void changeBinding(() => onAdd(value, { skipAvailabilityCheck: value === "MouseMiddle" })); }} />
    <HotkeyList values={hotkeys} emptyLabel={t("not_set")} removeLabel={t("delete")}
      disabled={disabled || saving} canRemove={canRemove}
      onRemove={value => { void changeBinding(() => onRemove(value)); }} />
    {error && <p className="hotkey-recording-error" role="alert">{error}</p>}
  </div>;
}
