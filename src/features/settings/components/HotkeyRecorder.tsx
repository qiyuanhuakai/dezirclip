import { useEffect, useState } from "react";
import { invoke } from "../../../shared/lib/desktopInvoke";
import { HotkeyBinding } from "@deziro/ui-react/components/HotkeyBinding";
import { useHotkeyRecording } from "../../../shared/hooks/useHotkeyRecording";
import { hotkeyChord } from "../utils/hotkeyChord";

interface HotkeyRecorderProps {
    readonly t: (key: string) => string;
    readonly labelKey: string;
    readonly hotkey: string;
    readonly recording: boolean;
    readonly setRecording: (val: boolean) => void;
    readonly onHotkeyChange: (val: string) => void | Promise<void>;
    readonly onClear?: () => void;
    readonly clearLabel?: string;
    readonly description?: string;
    readonly disabled?: boolean;
    readonly placeholder?: string;
    readonly allowMiddleMouse?: boolean;
}
const HotkeyRecorder = ({ t, labelKey, hotkey, recording, setRecording, onHotkeyChange, onClear, clearLabel, description, disabled = false, placeholder, allowMiddleMouse = true }: HotkeyRecorderProps) => {
    const [recordingError, setRecordingError] = useState<string | null>(null);
    const [saving, setSaving] = useState(false);
    const blocked = disabled || saving;
    useEffect(() => {
        if (recording) return () => setRecording(false);
    }, [recording, setRecording]);
    const { record, cancel, nativeReady } = useHotkeyRecording({ recording: recording && !blocked,
        onRecorded: async value => {
            if (value === "MouseMiddle" && !allowMiddleMouse) {
                setRecordingError(t("hotkey_unavailable"));
                return;
            }
            setSaving(true);
            try { await onHotkeyChange(value); }
            catch (error) { setRecordingError(t("hotkey_register_failed") + String(error)); }
            finally { setSaving(false); }
        },
        onCancelled: () => setRecording(false),
        onError: error => setRecordingError(t("hotkey_register_failed") + String(error)),
    });
    const startRecording = () => {
        if (blocked) return;
        setRecordingError(null);
        setRecording(true);
        invoke("focus_clipboard_window").catch(console.error);
    };
    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (blocked || e.repeat) return;
        if (!recording) {
            if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                e.stopPropagation();
                startRecording();
            }
            return;
        }
        e.preventDefault();
        e.stopPropagation();
        if (e.key === "Escape" && !e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey) {
            cancel();
            return;
        }
        const newHotkey = hotkeyChord(e);
        if (!newHotkey) return;
        record(newHotkey);
    };

    return <HotkeyBinding label={t(labelKey)} value={hotkey} placeholder={placeholder || t("not_set")} unset={!placeholder}
        recordingLabel={t("waiting_for_input")} recording={recording} busy={saving || (recording && !nativeReady)} disabled={blocked}
        onClear={onClear} error={recordingError} helpLabel={t("settings_help")} description={description} clearLabel={clearLabel}
        onClick={event => { startRecording(); event.currentTarget.focus(); }} onKeyDown={handleKeyDown}
        onMouseDown={event => {
            if (!recording || blocked || event.button !== 1) return;
            event.preventDefault();
            if (allowMiddleMouse) record("MouseMiddle");
        }}
        onBlur={() => setRecording(false)} />;
};

export default HotkeyRecorder;
