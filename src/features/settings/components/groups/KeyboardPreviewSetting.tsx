import { SettingsToggleField } from "@deziro/ui-react/components/SettingsToggle";
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
        <SettingsToggleField detail id="preview-keyboard-enabled" label={t("preview_keyboard_enabled")} helpLabel={t("settings_help")}
            description={t("preview_keyboard_enabled_hint")} checked={preferences.keyboardEnabled}
            disabled={!arrowKeySelection || !ready || saving} onChange={event => { void change(event.target.checked); }} />
        {(failed || error) && <p className="hint" role="alert">{t("preview_preferences_failed")}</p>}
    </NativeFeatureBoundary>;
}
