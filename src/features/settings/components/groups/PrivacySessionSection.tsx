import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { usePrivacySession } from "../../../../shared/hooks/usePrivacySession";
import { invokePrivacyBound, refreshPrivacyState, setPrivacySession } from "../../../../shared/lib/privacySession";
import "../../../../styles/components/privacy.css";
import ThemedSelect from "../ThemedSelect";
import HotkeyListEditor from "../HotkeyListEditor";
import SettingsDisclosure from "../SettingsDisclosure";

interface PrivacyRule { id: string; kind: string; source_app: string | null; created_at_ms: number }
export function PrivacySessionSection({ t, appSettings }: {
  t: (key: string) => string; appSettings: Record<string, string>;
}) {
  const state = usePrivacySession();
  const [duration, setDuration] = useState("900000");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [hotkey, setHotkey] = useState(appSettings["app.privacy_session_hotkey"] || "");
  const [rules, setRules] = useState<PrivacyRule[]>([]);
  const [rulesFailed, setRulesFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [recording, setRecording] = useState(false);
  const ready = state.phase === "ready";
  const disabled = !ready || state.pending || busy;
  const active = state.snapshot?.session.active === true && ready;

  useEffect(() => { setHotkey(appSettings["app.privacy_session_hotkey"] || ""); }, [appSettings["app.privacy_session_hotkey"]]);
  useEffect(() => {
    let disposed = false;
    setRules([]);
    if (ready) {
      setRulesFailed(false);
      void invokePrivacyBound<PrivacyRule[]>("get_privacy_rules").then(value => {
        if (!disposed) setRules(value);
      }).catch(() => { if (!disposed) setRulesFailed(true); });
    }
    return () => { disposed = true; };
  }, [ready, state.generation, attempt]);

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
  const removeRule = async (ruleId: string) => {
    const epoch = state.snapshot?.privacy_epoch;
    if (epoch === undefined) return;
    setError(null);
    setBusy(true);
    try { await invoke("remove_privacy_rule", { ruleId, expectedEpoch: epoch }); }
    catch { setError(t("privacy_action_failed")); }
    finally { await refreshPrivacyState(); setBusy(false); }
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
    <SettingsDisclosure className="privacy-rule-list" title={t("privacy_rules_title")}>
      <p>{t("privacy_rules_hint")}</p>
      {rulesFailed ? <button type="button" className="setting-btn" onClick={() => setAttempt(value => value + 1)}>{t("privacy_retry")}</button>
        : rules.length === 0 ? <p>{t("privacy_rules_empty")}</p>
          : <ul>{rules.map(rule => <li key={rule.id}>
            <span>{t(`privacy_rule_${rule.kind}`)}{rule.source_app ? ` · ${rule.source_app}` : ""}</span>
            <button type="button" className="setting-btn" disabled={disabled} onClick={() => { void removeRule(rule.id); }}>{t("privacy_rule_remove")}</button>
          </li>)}</ul>}
    </SettingsDisclosure>
    {error && <p role="alert" className="privacy-action-error">{error}</p>}
  </section>;
}
