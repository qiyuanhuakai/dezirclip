import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { usePrivacySession } from "../../../../shared/hooks/usePrivacySession";
import { refreshPrivacyState, setPrivacySession } from "../../../../shared/lib/privacySession";
import "../../../../styles/components/privacy.css";
import HotkeyRecorder from "../HotkeyRecorder";

export function PrivacySessionSection({ t, appSettings }: {
  t: (key: string) => string; appSettings: Record<string, string>;
}) {
  const state = usePrivacySession();
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [hotkey, setHotkey] = useState(appSettings["app.privacy_session_hotkey"] || "");
  const [recording, setRecording] = useState(false);
  const ready = state.phase === "ready";
  const disabled = !ready || state.pending || busy;
  const active = state.snapshot?.session.active === true && ready;

  useEffect(() => { setHotkey(appSettings["app.privacy_session_hotkey"] || ""); }, [appSettings["app.privacy_session_hotkey"]]);


  const changeSession = async (enabled: boolean) => {
    if (disabled) return;
    setError(null);
    setBusy(true);
    try { await setPrivacySession(enabled, null); }
    catch { setError(t("privacy_action_failed")); }
    finally { setBusy(false); }
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
      <div className="item-label-group"><label className="item-label" id="privacy-session-heading" htmlFor="privacy-session-enabled">{t("privacy_session_title")}</label></div>
      <label className="switch">
        <input id="privacy-session-enabled" className="cb" type="checkbox" checked={active} disabled={disabled}
          onChange={event => { void changeSession(event.target.checked); }} />
        <div className="toggle"><div className="left" /><div className="right" /></div>
      </label>
    </div>
    <p className="settings-description">{t("privacy_session_hint")}</p>
    {active && state.snapshot?.session.expires_at_ms !== null && state.snapshot?.session.expires_at_ms !== undefined
      && <p>{t("privacy_session_until")} {new Date(state.snapshot.session.expires_at_ms).toLocaleString()}</p>}
    {!ready && <div className="setting-item">
      <span className="settings-inline-note" role="status">{t(state.phase === "loading" ? "privacy_state_loading" : "privacy_state_unavailable")}</span>
      <button type="button" className="setting-btn" disabled={state.phase === "loading"}
        onClick={() => { void refreshPrivacyState(); }}>{t("privacy_retry")}</button>
    </div>}
    <HotkeyRecorder t={t} labelKey="privacy_session_hotkey" hotkey={hotkey}
      recording={recording} setRecording={setRecording} disabled={disabled}
      onHotkeyChange={value => { void saveHotkey(value); }} onClear={() => { void saveHotkey(""); }} />

    {error && <p role="alert" className="privacy-action-error">{error}</p>}
  </section>;
}
