import { useCallback, useState } from "react";
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
    readonly onHotkeyChange: (val: string) => void;
    readonly onClear?: () => void;
    readonly disabled?: boolean;
    readonly placeholder?: string;
}
const HotkeyRecorder = ({ t, labelKey, hotkey, recording, setRecording, onHotkeyChange, onClear, disabled = false, placeholder }: HotkeyRecorderProps) => {
    const [recordingError, setRecordingError] = useState<string | null>(null);
    const { record, cancel, nativeReady } = useHotkeyRecording({ recording: recording && !disabled, onRecorded: onHotkeyChange,
        onCancelled: () => setRecording(false),
        onError: error => setRecordingError(t("hotkey_register_failed") + String(error)),
    });
    const startRecording = () => {
        if (disabled) return;
        setRecordingError(null);
        setRecording(true);
        invoke("focus_clipboard_window").catch(console.error);
    };
    const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
        if (disabled) return;
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
        if (e.key === "Escape") {
            cancel();
            return;
        }
        const newHotkey = hotkeyChord(e);
        if (!newHotkey) return;
        record(newHotkey);
    }, [cancel, disabled, record, recording, setRecording]);

    return <HotkeyBinding label={t(labelKey)} value={hotkey} placeholder={placeholder || t("not_set")} unset={!placeholder}
        recordingLabel={t("waiting_for_input")} recording={recording} busy={recording && !nativeReady} disabled={disabled}
        onClear={onClear} error={recordingError} clearLabel={`${t("delete")} ${t(labelKey)}`} clearTitle={`${t("delete")} ${hotkey}`}
        onClick={event => { startRecording(); event.currentTarget.focus(); }} onKeyDown={handleKeyDown}
        onMouseDown={event => { if (recording && !disabled && event.button === 1) { event.preventDefault(); record("MouseMiddle"); } }}
        onBlur={() => setRecording(false)} />;
};

export default HotkeyRecorder;
