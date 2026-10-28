import { useRef, useState } from "react";
import { usePreviewPreferences } from "../../../../shared/hooks/usePreviewPreferences";
import { savePreviewPreference } from "../../../../shared/lib/previewPreferenceClient";
import NativeFeatureBoundary from "../NativeFeatureBoundary";

export default function KeyboardPreviewSetting({ t, arrowKeySelection }: {
    t: (key: string) => string;
    arrowKeySelection: boolean;
}) {
    const { preferences, ready, error } = usePreviewPreferences();
    const [saving, setSaving] = useState(false);
    const [failed, setFailed] = useState(false);
    const inFlight = useRef(false);
    const change = async (enabled: boolean) => {
        if (inFlight.current) return;
        inFlight.current = true;
        setSaving(true);
        setFailed(false);
        try { await savePreviewPreference("keyboardEnabled", enabled); }
        catch { setFailed(true); }
        finally { inFlight.current = false; setSaving(false); }
    };
    return <NativeFeatureBoundary t={t}>
        <div className="setting-item setting-item--detail">
            <div className="item-label-group"><label className="item-label" htmlFor="preview-keyboard-enabled">{t("preview_keyboard_enabled")}</label></div>
            <label className="switch">
                <input id="preview-keyboard-enabled" className="cb" type="checkbox" checked={preferences.keyboardEnabled}
                    disabled={!arrowKeySelection || !ready || saving} onChange={event => { void change(event.target.checked); }} />
                <div className="toggle"><div className="left" /><div className="right" /></div>
            </label>
        </div>
        {(failed || error) && <p className="hint" role="alert">{t("preview_preferences_failed")}</p>}
    </NativeFeatureBoundary>;
}
