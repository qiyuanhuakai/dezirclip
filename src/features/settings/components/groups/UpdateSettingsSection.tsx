import { SettingsToggleField } from "@deziro/ui-react/components/SettingsToggle";
import { SettingsField } from "@deziro/ui-react/components/SettingsField";
import { SettingsChoices } from "@deziro/ui-react/components/SettingsChoices";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { SettingHelp, SettingsLabel } from "../SettingHelp";
import SettingsDisclosure from "../SettingsDisclosure";
import { UpdateAction, UpdateActions, UpdateProgress, UpdateVersionRow } from "@deziro/ui-react/components/SettingsUpdate";
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
                <SettingHelp label={t("updater_title")}>
                    {t(supportKey(state.support?.reason))}
                    {state.phase === "idle" && <><br />{t("updater_not_checked")}</>}
                </SettingHelp>
            </h4>
            <SettingsToggleField label={t("updater_auto_check")} helpLabel={t("settings_help")}
                description={t("updater_auto_hint")} checked={state.autoCheck} disabled={disabled}
                onChange={event => void saveUpdatePreference("autoCheck", event.target.checked)} />
            <SettingsField className="update-channel-row">
                <SettingsLabel label={t("updater_channel")} hintKey="updater_channel"
                    hint={t(state.channel === "stable" ? "updater_stable_hint" : "updater_preview_hint")} />
                <SettingsChoices segmented label={t("updater_channel")} value={state.channel} disabled={disabled}
                    options={[{ value: "stable", label: "Stable" }, { value: "preview", label: "Preview" }]}
                    onChange={channel => { setConfirmationId(null); void saveUpdatePreference("channel", channel); }} />
            </SettingsField>
            <UpdateVersionRow label={t("updater_current_version")} version={state.currentVersion} />
            {channelVersion && <UpdateVersionRow label={t("updater_channel_version")} version={channelVersion} />}
            <UpdateActions>
                <UpdateAction action="check" disabled={disabled}
                    onClick={() => { setConfirmationId(null); void checkForUpdate(); }}>
                    {t(state.phase === "checking" ? "updater_checking" : "updater_check_now")}
                </UpdateAction>
                {available && state.support?.canInstall && (
                    <UpdateAction action="install" disabled={disabled} onClick={() => setConfirmationId(available.id)}>
                        {t("updater_install")}
                    </UpdateAction>
                )}
                <UpdateAction action="releases" onClick={() => void openUpdateRelease()}>
                    {t(available ? "updater_open_release" : "updater_all_releases")}
                </UpdateAction>
            </UpdateActions>
            {installPhase && <UpdateProgress
                status={t(state.phase === "downloading" ? "updater_downloading" : state.phase === "verified" ? "updater_verified" : "updater_installing")}
                amount={state.phase === "downloading" ? percent === null ? `${(state.downloaded / 1048576).toFixed(1)} MiB` : `${percent}%` : "100%"}
                percent={state.phase === "downloading" ? percent : 100}
                label={t("updater_download_progress")} verified={state.verified ? t("updater_verified") : undefined} />}
            <div className="update-status" role="status" aria-live="polite">
                {state.phase === "current" && <p>{t("updater_current")}</p>}
                {state.phase === "ahead" && <p>{t("updater_ahead")}</p>}
                {state.phase === "unpublished" && <p>{t("updater_unpublished")}</p>}
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
