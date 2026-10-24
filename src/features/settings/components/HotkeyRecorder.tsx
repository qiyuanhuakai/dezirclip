import { useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

interface HotkeyRecorderProps {
    readonly t: (key: string) => string;
    readonly labelKey: string;
    readonly hotkey: string;
    readonly recording: boolean;
    readonly setRecording: (val: boolean) => void;
    readonly onHotkeyChange: (val: string) => void;
    readonly disabled?: boolean;
}

const buildHotkey = (e: React.KeyboardEvent): string | null => {
    const key = e.key === " " ? "Space" : e.key.toUpperCase();
    if (["CONTROL", "SHIFT", "ALT", "META"].includes(key)) return null;
    const modifiers = [];
    if (e.ctrlKey) modifiers.push("Ctrl");
    if (e.shiftKey) modifiers.push("Shift");
    if (e.altKey) modifiers.push("Alt");
    if (e.metaKey) modifiers.push("Win");
    return [...modifiers, key].join("+");
};

const HotkeyRecorder = ({ t, labelKey, hotkey, recording, setRecording, onHotkeyChange, disabled = false }: HotkeyRecorderProps) => {
    const parts = hotkey ? hotkey.split("+") : [];
    const startRecording = () => {
        if (disabled) return;
        setRecording(true);
        invoke("focus_clipboard_window").catch(console.error);
    };
    const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
        if (disabled) return;
        if (!recording) {
            if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                e.stopPropagation();
                setRecording(true);
                invoke("focus_clipboard_window").catch(console.error);
            }
            return;
        }
        e.preventDefault();
        e.stopPropagation();
        if (e.key === "Escape") {
            setRecording(false);
            return;
        }
        const newHotkey = buildHotkey(e);
        if (!newHotkey) return;
        onHotkeyChange(newHotkey);
        setRecording(false);
    }, [disabled, onHotkeyChange, recording, setRecording]);

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
            onBlur={() => setRecording(false)}
        >
            {recording && !disabled ? <div className="key-cap key-cap--placeholder">{t("waiting_for_input")}</div>
                : parts.length > 0 ? parts.map((part, index) => <div key={`${part}-${index}`} className="key-cap">{part}</div>)
                    : <div className="key-cap key-cap--placeholder key-cap--unset">{t("not_set")}</div>}
        </div>
    </div>;
};

export default HotkeyRecorder;
