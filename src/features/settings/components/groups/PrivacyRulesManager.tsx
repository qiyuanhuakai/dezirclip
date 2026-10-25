import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { usePrivacySession } from "../../../../shared/hooks/usePrivacySession";
import { beginPrivacyAccess, invokePrivacyBound, refreshPrivacyState } from "../../../../shared/lib/privacySession";
import "../../../../styles/components/privacy.css";

interface PrivacyRule {
  id: string;
  kind: "exact_exception" | "ignore_source" | "protect_source";
  source_app: string | null;
  created_at_ms: number;
}
const ruleLabels = {
  exact_exception: "privacy_rule_exact_exception",
  ignore_source: "privacy_rule_ignore_source",
  protect_source: "privacy_rule_protect_source",
};
const ruleScopes = {
  exact_exception: "privacy_rule_scope_exact_exception",
  ignore_source: "privacy_rule_scope_ignore_source",
  protect_source: "privacy_rule_scope_protect_source",
};

export default function PrivacyRulesManager({ t }: { t: (key: string) => string }) {
  const state = usePrivacySession();
  const [rules, setRules] = useState<PrivacyRule[]>([]);
  const [load, setLoad] = useState<"loading" | "ready" | "failed">("loading");
  const [attempt, setAttempt] = useState(0);
  const [pendingDelete, setPendingDelete] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const inFlight = useRef(false);
  const ready = state.phase === "ready";
  const disabled = !ready || state.pending || busy;

  useEffect(() => {
    let disposed = false;
    setRules([]);
    setPendingDelete(null);
    setLoad("loading");
    if (ready) {
      void invokePrivacyBound<PrivacyRule[]>("get_privacy_rules").then(value => {
        if (!disposed) { setRules(value); setLoad("ready"); }
      }).catch(() => { if (!disposed) setLoad("failed"); });
    }
    return () => { disposed = true; };
  }, [ready, state.generation, attempt]);

  const remove = async (ruleId: string) => {
    const access = beginPrivacyAccess();
    if (disabled || !access || inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    setError(false);
    try {
      await invoke("remove_privacy_rule", { ruleId, expectedEpoch: access.epoch });
      setPendingDelete(null);
      setAttempt(value => value + 1);
    } catch { setError(true); }
    finally { await refreshPrivacyState(); inFlight.current = false; setBusy(false); }
  };

  return <section id="privacy-rule-manager" className="privacy-rule-manager" tabIndex={-1} aria-labelledby="privacy-rules-heading">
    <div className="setting-item settings-section-title">
      <div className="item-label-group"><span id="privacy-rules-heading" className="item-label">{t("privacy_rules_title")}</span></div>
      {load === "ready" && <span className="settings-inline-note">{rules.length}</span>}
    </div>
    <p className="settings-description">{t("privacy_rules_hint")}</p>
    <p className="settings-description">{t("privacy_rules_single_hint")}</p>
    {!ready ? <p role="status" className="settings-description">{t("privacy_state_unavailable")}</p>
      : load === "loading" ? <p role="status" className="settings-description">{t("privacy_rules_loading")}</p>
      : load === "failed" ? <p role="alert" className="privacy-action-error">{t("privacy_rules_load_failed")}</p>
      : rules.length === 0 ? <p className="settings-description">{t("privacy_rules_empty")}</p>
      : <ul className="privacy-rule-grid">{rules.map(rule => {
        const source = rule.source_app || t("privacy_source_unknown");
        const exact = rule.kind === "exact_exception";
        const created = new Date(rule.created_at_ms);
        const validDate = Number.isFinite(created.getTime());
        return <li key={rule.id} className="privacy-rule-card">
          <div className="privacy-rule-heading">
            <span className="item-label">{t(ruleLabels[rule.kind])}</span>
            <span className="privacy-rule-source">{source}</span>
          </div>
          <p className="privacy-rule-scope">{t(ruleScopes[rule.kind])}</p>
          <div className="privacy-rule-metadata">
            <span title={rule.id}>{t("privacy_rule_identifier")} {rule.id.slice(0, 8)}</span>
            <span>{t("privacy_rule_created")} <time dateTime={validDate ? created.toISOString() : undefined}>{validDate ? created.toLocaleString() : "—"}</time></span>
          </div>
          {pendingDelete === rule.id ? <div className="privacy-rule-confirm" role="group" aria-label={t("privacy_rule_delete_title")}>
            <p>{t(exact ? "privacy_rule_delete_exact_confirm" : "privacy_rule_delete_source_confirm").replace("{source}", source)}</p>
            <div className="setting-btn-group">
              <button type="button" className="setting-btn" disabled={busy} onClick={() => setPendingDelete(null)}>{t("cancel")}</button>
              <button type="button" className="setting-btn" disabled={disabled} onClick={() => { void remove(rule.id); }}>{t(exact ? "privacy_rule_revoke_exception" : "privacy_rule_remove")}</button>
            </div>
          </div> : <button type="button" className="setting-btn" disabled={disabled}
            onClick={() => { setPendingDelete(rule.id); setError(false); }}>{t(exact ? "privacy_rule_revoke_exception" : "privacy_rule_remove")}</button>}
        </li>;
      })}</ul>}
    {error && <p role="alert" className="privacy-action-error">{t("privacy_rules_action_failed")}</p>}
    {(load === "failed" || error || !ready) && <button type="button" className="setting-btn" disabled={busy || state.phase === "loading"}
      onClick={() => { setError(false); setAttempt(value => value + 1); void refreshPrivacyState(); }}>{t("privacy_retry")}</button>}
  </section>;
}
