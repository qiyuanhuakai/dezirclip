import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useHotkeyRecording } from "../../../shared/hooks/useHotkeyRecording";
import { hotkeyChord } from "../utils/hotkeyChord";

interface HotkeyRecorderProps {
    readonly t: (key: string) => string;
    readonly labelKey: string;
    readonly hotkey: string;
    readonly recording: boolean;
    readonly setRecording: (val: boolean) => void;
    readonly onHotkeyChange: (val: string) => void;
    readonly disabled?: boolean;
    readonly placeholder?: string;
}
const HotkeyRecorder = ({ t, labelKey, hotkey, recording, setRecording, onHotkeyChange, disabled = false, placeholder }: HotkeyRecorderProps) => {
    const [recordingError, setRecordingError] = useState<string | null>(null);
    const { record, cancel } = useHotkeyRecording({ recording: recording && !disabled, onRecorded: onHotkeyChange,
        onCancelled: () => setRecording(false),
        onError: error => setRecordingError(t("hotkey_register_failed") + String(error)),
    });
    const parts = hotkey ? hotkey.split("+") : [];
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

    return <div className="setting-item setting-item--detail hotkey-recorder">
        <div className="item-label-group"><span className="item-label">{t(labelKey)}</span></div>
        <div
            className={`key-group ${recording && !disabled ? "recording" : ""}`}
            role="button"
            aria-label={t(labelKey)}
            aria-pressed={recording && !disabled}
            aria-disabled={disabled}
            tabIndex={disabled ? -1 : 0}
            onClick={event => { startRecording(); event.currentTarget.focus(); }}
            onKeyDown={handleKeyDown}
            onMouseDown={event => { if (recording && !disabled && event.button === 1) { event.preventDefault(); record("MouseMiddle"); } }}
            onBlur={() => setRecording(false)}
        >
            {recording && !disabled ? <div className="key-cap key-cap--placeholder">{t("waiting_for_input")}</div>
                : parts.length > 0 ? parts.map((part, index) => <div key={`${part}-${index}`} className="key-cap">{part}</div>)
                    : <div className={`key-cap key-cap--placeholder ${placeholder ? "" : "key-cap--unset"}`}>{placeholder || t("not_set")}</div>}
        </div>
        {recordingError && <p className="hotkey-recording-error" role="alert">{recordingError}</p>}
    </div>;
};

export default HotkeyRecorder;
