import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { SettingHelp, SettingsLabel } from "../SettingHelp";
import SettingsDisclosure from "../SettingsDisclosure";
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
    const channelVersion = state.channelVersion ?? available?.version;
    const selectedChannelName = t(state.channel === "stable" ? "updater_channel_stable_name" : "updater_channel_preview_name");
    const confirming = confirmationId !== null && confirmationId === available?.id;
    const percent = state.total && state.total > 0 ? Math.min(100, Math.floor(state.downloaded / state.total * 100)) : null;
    const installPhase = ["downloading", "verified", "installing"].includes(state.phase);
    const dialog = useRef<HTMLDialogElement>(null);
    const showConfirmation = confirming && !disabled;
    useEffect(() => {
        const element = dialog.current;
        if (showConfirmation) element?.showModal();
        else element?.close();
    }, [showConfirmation]);

    return (
        <section className="update-settings" aria-labelledby="update-settings-title">
            <h4 id="update-settings-title" className="settings-label-line">{t("updater_title")}
                {state.support?.canInstall && <SettingHelp label={t("updater_title")}>{t(supportKey(state.support.reason))}</SettingHelp>}
            </h4>
            <div className="setting-item">
                <SettingsLabel label={t("updater_auto_check")} hintKey="updater_auto" />
                <label className="switch">
                    <input className="cb" type="checkbox" checked={state.autoCheck} disabled={disabled}
                        aria-label={t("updater_auto_check")}
                        onChange={event => void saveUpdatePreference("autoCheck", event.target.checked)} />
                    <div className="toggle"><div className="left" /><div className="right" /></div>
                </label>
            </div>
            <div className="setting-item update-channel-row">
                <SettingsLabel label={t("updater_channel")} hintKey="updater_channel"
                    hint={t(state.channel === "stable" ? "updater_stable_hint" : "updater_preview_hint")} />
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
            <p className="update-note">{t("updater_selected_channel")} <strong>{selectedChannelName}</strong></p>
            <div className="update-version-row">
                <span>{t("updater_current_version")}</span><strong>{state.currentVersion ? `v${state.currentVersion}` : "—"}</strong>
            </div>
            {channelVersion && <div className="update-version-row">
                <span>{t("updater_channel_version")}</span><strong>v{channelVersion}</strong>
            </div>}
            <div className="setting-btn-group update-actions">
                <button type="button" className="btn-icon settings-action" disabled={disabled}
                    onClick={() => { setConfirmationId(null); void checkForUpdate(); }}>
                    <RefreshCw size={14} />{t(state.phase === "checking" ? "updater_checking" : "updater_check_now")}
                </button>
                {available && state.support?.canInstall && (
                    <button type="button" className="btn-icon settings-action" disabled={disabled} onClick={() => setConfirmationId(available.id)}>
                        <Download size={14} />{t("updater_install")}
                    </button>
                )}
                {installPhase && <div className="update-download" role="status" aria-live="polite">
                    <div className="settings-range-header">
                        <span>{t(state.phase === "downloading" ? "updater_downloading" : state.phase === "verified" ? "updater_verified" : "updater_installing")}</span>
                        <span>{state.phase === "downloading" ? percent === null ? `${(state.downloaded / 1048576).toFixed(1)} MiB` : `${percent}%` : "100%"}</span>
                    </div>
                    <div className={`update-progress${state.phase === "downloading" && percent === null ? " is-indeterminate" : ""}`}
                        role="progressbar" aria-label={t("updater_download_progress")} aria-valuemin={0} aria-valuemax={100}
                        aria-valuenow={state.phase === "downloading" ? percent ?? undefined : 100}>
                        <span className="update-progress-fill" style={{ transform: `scaleX(${state.phase === "downloading" ? (percent ?? 35) / 100 : 1})` }} />
                    </div>
                    {state.verified && <p className="update-verified"><ShieldCheck size={14} />{t("updater_verified")}</p>}
                </div>}
                <button type="button" className="btn-icon settings-action" onClick={() => void openUpdateRelease()}>
                    <ExternalLink size={14} />{t(available ? "updater_open_release" : "updater_all_releases")}
                </button>
            </div>
            {!state.support?.canInstall && <p className="update-note">{t(supportKey(state.support?.reason))}</p>}
            <div className="update-status" role="status" aria-live="polite">
                {state.phase === "current" && <p>{t("updater_current")}</p>}
                {state.phase === "ahead" && <p>{t("updater_ahead")}</p>}
                {state.phase === "unpublished" && <p>{t("updater_unpublished")}</p>}
                {state.phase === "idle" && <p className="update-note">{t("updater_not_checked")}</p>}
                {state.phase === "available" && <p>{t("updater_found")}</p>}
                {state.checkedAt && <p className="update-note">{t("updater_checked_at")} {new Date(state.checkedAt).toLocaleTimeString()}</p>}
                {state.channel === "preview" && state.phase !== "unpublished" && state.unpublishedChannels.includes("preview")
                    && <p className="update-note">{t("updater_preview_index_missing")}</p>}
            </div>
            {state.errorKey && <p className="update-error" role="alert">{t(state.errorKey)}</p>}
            <dialog ref={dialog} className="settings-update-dialog modal-content confirm-dialog" aria-labelledby="update-confirm-title"
                onCancel={() => setConfirmationId(null)} onClose={() => setConfirmationId(null)}>
                <h4 id="update-confirm-title">{t("updater_confirm_title")} v{available?.version}</h4>
                <p>{t("updater_confirm_message")}</p>
                <div className="setting-btn-group">
                    <button type="button" className="btn-icon settings-action" autoFocus onClick={() => setConfirmationId(null)}>{t("updater_cancel")}</button>
                    <button type="button" className="btn-icon settings-action" onClick={() => {
                        const id = confirmationId;
                        setConfirmationId(null);
                        if (id && !disabled && id === available?.id) void installAvailableUpdate(id);
                    }}>{t("updater_confirm_install")}</button>
                </div>
            </dialog>
            {available && <SettingsDisclosure className="update-release-notes" title={t("updater_release_notes")}>
                <pre>{available.notes || t("updater_no_notes")}</pre>
            </SettingsDisclosure>}
        </section>
    );
};

export default UpdateSettingsSection;
