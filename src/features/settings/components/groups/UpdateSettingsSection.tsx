import { useState, useSyncExternalStore } from "react";
import { Download, ExternalLink, RefreshCw, ShieldCheck } from "lucide-react";
import { checkForUpdate, getUpdaterSnapshot, installAvailableUpdate, openUpdateRelease, saveUpdatePreference, subscribeUpdater } from "../../utils/updaterClient";

const supportKey = (reason: string | undefined): string => {
    switch (reason) {
        case "nsis_in_place": return "updater_support_nsis";
        case "appimage_in_place": return "updater_support_appimage";
        case "appimage_not_writable": return "updater_support_readonly";
        case "use_package_manager": return "updater_support_package";
        case "replace_portable_manually": return "updater_support_portable";
        default: return "updater_support_unavailable";
    }
};

const UpdateSettingsSection = ({ t }: { t: (key: string) => string }) => {
    const state = useSyncExternalStore(subscribeUpdater, getUpdaterSnapshot);
    const [confirmationId, setConfirmationId] = useState<string | null>(null);
    const busy = ["checking", "downloading", "verified", "installing"].includes(state.phase);
    const disabled = !state.ready || busy || state.saving;
    const available = state.available;
    const confirming = confirmationId !== null && confirmationId === available?.id;
    const percent = state.total && state.total > 0 ? Math.min(100, Math.floor(state.downloaded / state.total * 100)) : null;
    const installPhase = ["downloading", "verified", "installing"].includes(state.phase);

    return (
        <section className="update-settings" aria-labelledby="update-settings-title">
            <h4 id="update-settings-title">{t("updater_title")}</h4>
            <div className="setting-item">
                <div className="item-label-group">
                    <span className="item-label">{t("updater_auto_check")}</span>
                    <span className="hint">{t("updater_auto_hint")}</span>
                </div>
                <label className="switch">
                    <input className="cb" type="checkbox" checked={state.autoCheck} disabled={disabled}
                        aria-label={t("updater_auto_check")}
                        onChange={event => void saveUpdatePreference("autoCheck", event.target.checked)} />
                    <div className="toggle"><div className="left" /><div className="right" /></div>
                </label>
            </div>
            <div className="setting-item update-channel-row">
                <div className="item-label-group"><span className="item-label">{t("updater_channel")}</span></div>
                <div className="segmented-control" role="group" aria-label={t("updater_channel")}>
                    {(["stable", "preview"] as const).map(channel => (
                        <button type="button" key={channel} disabled={disabled} aria-pressed={state.channel === channel}
                            className={`segment-btn${state.channel === channel ? " active" : ""}`}
                            onClick={() => { setConfirmationId(null); void saveUpdatePreference("channel", channel); }}>
                            {channel === "stable" ? "Stable" : "Preview"}
                        </button>
                    ))}
                </div>
            </div>
            <p className="update-note">{t(state.channel === "stable" ? "updater_stable_hint" : "updater_preview_hint")}</p>
            <div className="update-version-row">
                <span>{t("updater_current_version")}</span><strong>{state.currentVersion ? `v${state.currentVersion}` : "—"}</strong>
            </div>
            {available && <div className="update-version-row">
                <span>{t("updater_available_version")}</span><strong>v{available.version}</strong>
            </div>}
            <div className="setting-btn-group update-actions">
                <button type="button" className="setting-btn" disabled={disabled}
                    onClick={() => { setConfirmationId(null); void checkForUpdate(); }}>
                    <RefreshCw size={14} />{t(state.phase === "checking" ? "updater_checking" : "updater_check_now")}
                </button>
                {available && state.support?.canInstall && !installPhase && (
                    <button type="button" className="setting-btn" disabled={disabled} onClick={() => setConfirmationId(available.id)}>
                        <Download size={14} />{t("updater_install")}
                    </button>
                )}
                <button type="button" className="setting-btn" onClick={() => void openUpdateRelease()}>
                    <ExternalLink size={14} />{t(available ? "updater_open_release" : "updater_all_releases")}
                </button>
            </div>
            <p className="update-note">{t(supportKey(state.support?.reason))}</p>
            <div className="update-status" role="status" aria-live="polite">
                {state.phase === "current" && <p>{t("updater_current")}</p>}
                {state.phase === "idle" && <p className="update-note">{t("updater_not_checked")}</p>}
                {state.phase === "available" && <p>{t("updater_found")}</p>}
                {state.checkedAt && <p className="update-note">{t("updater_checked_at")} {new Date(state.checkedAt).toLocaleTimeString()}</p>}
                {state.unpublishedChannels.length > 0 && <p className="update-note">{t("updater_missing_channel")} {state.unpublishedChannels.join(" / ")}</p>}
                {installPhase && <>
                    <p>{t(state.phase === "downloading" ? "updater_downloading" : state.phase === "verified" ? "updater_verified" : "updater_installing")}
                        {state.phase === "downloading" && (percent === null ? ` ${(state.downloaded / 1048576).toFixed(1)} MiB` : ` ${percent}%`)}</p>
                    <progress className="update-progress" aria-label={t("updater_download_progress")}
                        max={100} value={state.phase === "downloading" ? percent ?? undefined : 100} />
                    {state.verified && <p className="update-verified"><ShieldCheck size={14} />{t("updater_verified")}</p>}
                </>}
            </div>
            {state.errorKey && <p className="update-error" role="alert">{t(state.errorKey)}</p>}
            {confirming && !busy && <div className="update-confirm" role="group" aria-label={t("updater_confirm_title")}>
                <strong>{t("updater_confirm_title")} v{available?.version}</strong>
                <p>{t("updater_confirm_message")}</p>
                <div className="setting-btn-group">
                    <button type="button" className="setting-btn" onClick={() => setConfirmationId(null)}>{t("updater_cancel")}</button>
                    <button type="button" className="setting-btn" onClick={() => {
                        const id = confirmationId;
                        setConfirmationId(null);
                        if (id) void installAvailableUpdate(id);
                    }}>{t("updater_confirm_install")}</button>
                </div>
            </div>}
            {available && <details className="update-release-notes">
                <summary>{t("updater_release_notes")}</summary>
                <pre>{available.notes || t("updater_no_notes")}</pre>
            </details>}
        </section>
    );
};

export default UpdateSettingsSection;
