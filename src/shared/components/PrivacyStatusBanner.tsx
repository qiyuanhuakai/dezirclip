import { usePrivacySession } from "../hooks/usePrivacySession";
import { refreshPrivacyState, setPrivacySession } from "../lib/privacySession";
import "../../styles/components/privacy.css";

export function PrivacyStatusBanner({ t }: { t: (key: string) => string }) {
  const state = usePrivacySession();
  if (state.phase === "ready" && !state.snapshot?.session.active) return null;
  return <div className="privacy-status-banner" role="status">
    <span>{t(state.phase === "ready" ? "privacy_session_active" : state.phase === "loading" ? "privacy_state_loading" : "privacy_state_unavailable")}</span>
    {state.phase === "ready" ? <button type="button" disabled={state.pending}
      onClick={() => { void setPrivacySession(false).catch(() => undefined); }}>{t("privacy_session_end")}</button>
      : state.phase === "unavailable" && <button type="button" onClick={() => { void refreshPrivacyState(); }}>{t("privacy_retry")}</button>}
  </div>;
}
