import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { usePrivacySession } from "../../../../shared/hooks/usePrivacySession";
import { refreshPrivacyState, setPrivacySession } from "../../../../shared/lib/privacySession";
import "../../../../styles/components/privacy.css";
import ThemedSelect from "../ThemedSelect";
import HotkeyListEditor from "../HotkeyListEditor";

export function PrivacySessionSection({ t, appSettings }: {
  t: (key: string) => string; appSettings: Record<string, string>;
}) {
  const state = usePrivacySession();
  const [duration, setDuration] = useState("900000");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [hotkey, setHotkey] = useState(appSettings["app.privacy_session_hotkey"] || "");
  const [recording, setRecording] = useState(false);
  const ready = state.phase === "ready";
  const disabled = !ready || state.pending || busy;
  const active = state.snapshot?.session.active === true && ready;

  useEffect(() => { setHotkey(appSettings["app.privacy_session_hotkey"] || ""); }, [appSettings["app.privacy_session_hotkey"]]);


  const changeSession = async (enabled: boolean) => {
    setError(null);
    try { await setPrivacySession(enabled, enabled && duration !== "0" ? Number(duration) : null); }
    catch { setError(t("privacy_action_failed")); }
  };
  const saveHotkey = async (value: string): Promise<boolean> => {
    if (disabled) return false;
    setError(null);
    setBusy(true);
    try {
      if (value && value.toUpperCase() !== hotkey.toUpperCase()) await invoke<boolean>("test_hotkey_available", { hotkey: value });
      const persisted = await invoke<string>("set_privacy_session_hotkey", { hotkey: value });
      setHotkey(persisted);
      void emit("settings-changed").catch(console.error);
      return true;
    }
    catch (failure) {
      const detail = failure instanceof Error ? failure.message : String(failure ?? t("hotkey_unavailable"));
      setError(t("hotkey_register_failed") + detail);
      return false;
    }
    finally { setBusy(false); }
  };


  return <section className="privacy-session-section" aria-labelledby="privacy-session-heading">
    <div className="setting-item settings-section-title">
      <div className="item-label-group"><span className="item-label" id="privacy-session-heading">{t("privacy_session_title")}</span></div>
      <span className="privacy-session-state settings-inline-note" role="status">
        {t(ready ? active ? "privacy_session_active" : "privacy_session_inactive"
          : state.phase === "loading" ? "privacy_state_loading" : "privacy_state_unavailable")}
      </span>
    </div>
    <p className="settings-description">{t("privacy_session_hint")}</p>
    {active && state.snapshot?.session.expires_at_ms !== null && state.snapshot?.session.expires_at_ms !== undefined
      && <p>{t("privacy_session_until")} {new Date(state.snapshot.session.expires_at_ms).toLocaleString()}</p>}
    <div className="settings-section-grid">
    <div className="setting-item privacy-session-duration">
      <label htmlFor="privacy-session-duration">{t("privacy_session_duration")}</label>
      <ThemedSelect inputId="privacy-session-duration" value={duration} isDisabled={disabled} onChange={setDuration}
        options={[
          { value: "300000", label: t("privacy_duration_5m") },
          { value: "900000", label: t("privacy_duration_15m") },
          { value: "3600000", label: t("privacy_duration_1h") },
          { value: "0", label: t("privacy_duration_indefinite") },
        ]} />
    </div>
    <div className="setting-item privacy-session-controls"><div className="setting-btn-group">
      <button type="button" className="setting-btn" disabled={disabled} onClick={() => { void changeSession(true); }}>
        {t(active ? "privacy_session_renew" : "privacy_session_start")}</button>
      {active && <button type="button" className="setting-btn" disabled={disabled} onClick={() => { void changeSession(false); }}>{t("privacy_session_end")}</button>}
      {!ready && <button type="button" className="setting-btn" disabled={state.phase === "loading"}
        onClick={() => { void refreshPrivacyState(); }}>{t("privacy_retry")}</button>}
    </div></div>
    <HotkeyListEditor t={t} labelKey="privacy_session_hotkey" hotkeys={hotkey ? [hotkey] : []}
      recording={recording} setRecording={setRecording} disabled={disabled}
      onAdd={saveHotkey} onRemove={() => saveHotkey("")} />
    </div>

    {error && <p role="alert" className="privacy-action-error">{error}</p>}
  </section>;
}
