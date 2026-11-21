import { displayPath } from "../../utils/displayPath";
import { useEffect, useRef, useState } from "react";
import { open, save, ask, message } from "@tauri-apps/plugin-dialog";
import { invoke } from "../../../../shared/lib/desktopInvoke";
import { ChevronDown, ChevronRight } from "lucide-react";
import { SettingsDataPath, SettingsDataAction } from "@deziro/ui-react/components/SettingsData";
import { BackupActions, BackupChoices, BackupDialog, BackupField, BackupPreview, BackupReplaceConfirmation } from "@deziro/ui-react/components/BackupDialog";

import { beginPrivacyAccess, getMainPageStamp, subscribePrivacyInvalidation } from "../../../../shared/lib/privacySession";
import { usePrivacySession } from "../../../../shared/hooks/usePrivacySession";

interface DataSettingsGroupProps {
    t: (key: string) => string;
    collapsed: boolean;
    onToggle: () => void;
    dataPath: string;
}

interface ExportModalProps {
    t: (key: string) => string;
    onClose: () => void;
    onComplete: () => void;
}

interface ImportModalProps {
    t: (key: string) => string;
    onClose: () => void;
    onComplete: () => void;
}

interface BackupInspection {
    encrypted: boolean;
    entries: Array<{ content: string }>;
}

interface ProfileMoveReceipt {
    path: string;
    reconnect: boolean;
}

const ExportModal = ({ t, onClose, onComplete }: ExportModalProps) => {
    const [format, setFormat] = useState<"json" | "encrypted">("json");
    const [passphrase, setPassphrase] = useState("");
    const [passphraseConfirm, setPassphraseConfirm] = useState("");
    const [filePath, setFilePath] = useState("");
    const [passphraseError, setPassphraseError] = useState("");
    const [passphraseMismatchError, setPassphraseMismatchError] = useState("");
    const [exporting, setExporting] = useState(false);
    const [picking, setPicking] = useState(false);
    const [failure, setFailure] = useState("");

    const handleBrowse = async () => {
        setPicking(true);
        setFailure("");
        try {
            const path = await save({
                defaultPath: format === "json" ? "clipboard-export.json" : "clipboard-export.dzc",
                filters: format === "json"
                    ? [{ name: "JSON", extensions: ["json"] }]
                    : [{ name: "Encrypted Backup", extensions: ["dzc"] }],
            });
            if (path) setFilePath(path);
        } catch {
            setFailure(t("native_backup_picker_failed"));
        } finally {
            setPicking(false);
        }
    };

    const validate = (): boolean => {
        let valid = true;
        setPassphraseError("");
        setPassphraseMismatchError("");

        if (format === "encrypted") {
            if (Array.from(passphrase).length < 12) {
                setPassphraseError(t("data.export.passphrase_too_short"));
                valid = false;
            }
            if (passphrase !== passphraseConfirm) {
                setPassphraseMismatchError(t("data.export.passphrase_mismatch"));
                valid = false;
            }
        }
        return valid;
    };

    const handleExport = async () => {
        if (!validate() || !filePath) return;
        const access = beginPrivacyAccess("list");
        if (!access) {
            setFailure(t("native_backup_export_failed"));
            return;
        }
        setExporting(true);
        setFailure("");
        try {
            await invoke("export_to_file", {
                path: filePath,
                format,
                passphrase: format === "encrypted" ? passphrase : undefined,
                expectedEpoch: access.epoch,
            });
            if (access.isCurrent()) {
                onComplete();
                onClose();
            }
        } catch {
            setFailure(t("native_backup_export_failed"));
        } finally {
            setExporting(false);
        }
    };

    const busy = exporting || picking;
    const close = () => { if (!busy) onClose(); };
    return <BackupDialog title={t("data.export.title")} busy={busy} onClose={close}
        footer={<BackupActions cancel={t("cancel")} submit={t(busy ? "processing" : "data.export.title")}
            busy={busy} disabled={!filePath} onCancel={close} onSubmit={handleExport} />}>
        <BackupChoices label={t("data.export.choose_format")} value={format}
            options={[{ value: "json", label: t("data.export.json") }, { value: "encrypted", label: t("data.export.encrypted") }]}
            disabled={busy} onChange={next => {
                setFormat(next);
                setFilePath("");
                setPassphrase("");
                setPassphraseConfirm("");
                setPassphraseError("");
                setPassphraseMismatchError("");
                setFailure("");
            }} />
        {format === "encrypted" && <>
            <BackupField label={t("data.export.passphrase_prompt")} type="password" value={passphrase}
                onChange={e => setPassphrase(e.target.value)} placeholder="••••••••••••" autoComplete="new-password"
                disabled={busy} error={passphraseError} />
            <BackupField label={t("data.export.confirm_passphrase")} type="password" value={passphraseConfirm}
                onChange={e => setPassphraseConfirm(e.target.value)} placeholder="••••••••••••" autoComplete="new-password"
                disabled={busy} error={passphraseMismatchError} />
        </>}
        <BackupField label={t("data_path")} value={filePath} readOnly title={filePath}
            browse={t("browse_file")} onBrowse={handleBrowse} disabled={busy} />
        {failure && <p className="data-error" role="alert">{failure}</p>}
    </BackupDialog>;
};

const ImportModal = ({ t, onClose, onComplete }: ImportModalProps) => {
    const [mode, setMode] = useState<"merge" | "replace">("merge");
    const [replaceConfirmed, setReplaceConfirmed] = useState(false);
    const [filePath, setFilePath] = useState("");
    const [format, setFormat] = useState<"json" | "encrypted" | null>(null);
    const [passphrase, setPassphrase] = useState("");
    const [importing, setImporting] = useState(false);
    const [picking, setPicking] = useState(false);
    const [failure, setFailure] = useState("");
    const [passphraseError, setPassphraseError] = useState("");
    const [previewEntries, setPreviewEntries] = useState<Array<{ content: string }>>([]);

    const selection = useRef(0);
    useEffect(() => {
        const unsubscribe = subscribePrivacyInvalidation(scope => {
            setPreviewEntries([]);
            if (scope === "all") {
                selection.current++;
                setFilePath("");
                setFormat(null);
                setReplaceConfirmed(false);
                setPassphrase("");
                setPassphraseError("");
                setFailure("");
            }
        });
        return () => { selection.current++; unsubscribe(); };
    }, []);

    const handleBrowse = async () => {
        const request = ++selection.current;
        setPicking(true);
        try {
            const path = await open({ multiple: false, filters: [{ name: "Backup", extensions: ["json", "dzc", "tiez"] }] });
            if (!path || request !== selection.current) return;
            setFilePath(path);
            setFormat(null);
            setFailure("");
            setReplaceConfirmed(false);
            setPreviewEntries([]);
            setPassphrase("");
            setPassphraseError("");

            try {
                const access = beginPrivacyAccess("list");
                const previewAccess = beginPrivacyAccess();
                if (!access) throw new Error("PrivacyStateUnavailable");
                const inspection = await invoke<BackupInspection>("inspect_backup_file", { path, expectedEpoch: access.epoch });
                if (request !== selection.current || !access.isCurrent()) return;
                setFormat(inspection.encrypted ? "encrypted" : "json");
                if (previewAccess?.isCurrent()) setPreviewEntries(inspection.entries);
            } catch {
                if (request === selection.current) setFailure(t("native_backup_preview_failed"));
            }
        } catch {
            if (request === selection.current) setFailure(t("native_backup_picker_failed"));
        } finally {
            setPicking(false);
        }
    };

    const canSubmit = Boolean(filePath && format && (mode !== "replace" || replaceConfirmed));
    const handleImport = async () => {
        if (!canSubmit) return;
        if (format === "encrypted" && !passphrase) {
            setPassphraseError(t("native_backup_password_required"));
            return;
        }
        const access = beginPrivacyAccess("list");
        if (!access) {
            setFailure(t("native_backup_import_failed"));
            return;
        }
        setPassphraseError("");
        setFailure("");
        setImporting(true);
        try {
            await invoke("import_from_file", {
                path: filePath,
                mode,
                passphrase: format === "encrypted" ? passphrase : undefined,
                expectedEpoch: access.epoch,
            });
            if (access.isCurrent()) {
                onComplete();
                onClose();
            }
        } catch {
            setFailure(t("native_backup_import_failed"));
        } finally {
            setImporting(false);
        }
    };

    const busy = picking || importing;
    const close = () => { if (!busy) onClose(); };
    return <BackupDialog title={t("data.import.title")} busy={busy} onClose={close}
        footer={<BackupActions cancel={t("cancel")} submit={t(busy ? "processing" : "data.import.title")}
            busy={busy} disabled={!canSubmit} onCancel={close} onSubmit={handleImport} />}>
        <BackupChoices label={t("data.import.mode")} value={mode}
            options={[{ value: "merge", label: t("data.import.merge") }, { value: "replace", label: t("data.import.replace") }]}
            disabled={busy} onChange={next => {
                setMode(next);
                setReplaceConfirmed(false);
                if (format) setFailure("");
            }} />
        {mode === "replace" && <BackupReplaceConfirmation label={t("native_backup_replace_confirm")}
            checked={replaceConfirmed} disabled={busy || format === null} onChange={setReplaceConfirmed} />}
        <BackupField label={t("data_path")} value={filePath} readOnly title={filePath}
            browse={t("browse_file")} onBrowse={handleBrowse} disabled={busy} />
        {format === "encrypted" && <BackupField label={t("data.import.passphrase_prompt")} type="password"
            value={passphrase} onChange={e => setPassphrase(e.target.value)} placeholder="••••••••••••"
            autoComplete="off" disabled={busy} error={passphraseError} />}
        {previewEntries.length > 0 &&
            <BackupPreview label={t("data.import.preview")} entries={previewEntries.map(entry => entry.content)} />}
        {failure && <p className="data-error" role="alert">{failure}</p>}
    </BackupDialog>;
};

const DataSettingsGroup = ({ t, collapsed, onToggle, dataPath }: DataSettingsGroupProps) => {
    const [backupDialog, setBackupDialog] = useState<"export" | "import" | null>(null);
    const [completed, setCompleted] = useState<"export" | "import" | null>(null);
    const [movedProfile, setMovedProfile] = useState<ProfileMoveReceipt | null>(null);
    const [checkingProfile, setCheckingProfile] = useState(true);
    const [profileBusy, setProfileBusy] = useState(false);
    const [profileError, setProfileError] = useState("");
    const privacy = usePrivacySession();
    const privateSession = privacy.snapshot?.session.active === true;
    const profileLocked = checkingProfile || profileBusy || movedProfile !== null;
    const backupDisabled = privacy.phase !== "ready" || privateSession || profileLocked;
    useEffect(() => {
        let current = true;
        invoke<ProfileMoveReceipt | null>("get_pending_data_move").then(receipt => {
            if (current && receipt) setMovedProfile(receipt);
        }).catch(() => undefined).finally(() => {
            if (current) setCheckingProfile(false);
        });
        return () => { current = false; };
    }, []);
    useEffect(() => {
        if (privateSession) setBackupDialog(null);
    }, [privateSession]);

    const openBackup = (mode: "export" | "import") => {
        setCompleted(null);
        setBackupDialog(mode);
    };

    const restartProfile = async () => {
        setProfileError("");
        try {
            await invoke("relaunch");
        } catch (error) {
            setProfileError(t("data_move_retry") + " " + String(error));
        }
    };

    const changeDirectory = async () => {
        const access = beginPrivacyAccess("list");
        const stamp = getMainPageStamp();
        if (!access || !stamp || stamp.sessionActive || stamp.epoch !== access.epoch) return;
        setProfileBusy(true);
        setProfileError("");
        try {
            let newPath: string;
            try {
                const selected = await open({ directory: true, multiple: false, title: t("change_data_path"), defaultPath: dataPath });
                if (!selected || displayPath(selected) === displayPath(dataPath) || !access.isCurrent()) return;
                newPath = selected;
                const confirmed = await ask(t("data_move_confirm").replace("{path}", displayPath(newPath)), {
                    title: t("change_data_path"), kind: "warning", okLabel: t("confirm"), cancelLabel: t("cancel"),
                });
                if (!confirmed || !access.isCurrent() || getMainPageStamp()?.instanceId !== stamp.instanceId) return;
            } catch {
                setProfileError(t("native_profile_picker_failed"));
                return;
            }
            const receipt = await invoke<ProfileMoveReceipt | null>("set_data_path", {
                newPath, expectedEpoch: access.epoch, expectedHostInstanceId: stamp.instanceId,
            });
            if (!receipt) return;
            setMovedProfile(receipt);
            await message(t(receipt.reconnect ? "data_move_reconnect" : "data_move_success"),
                { title: t("notice"), kind: "info" }).catch(() => undefined);
            await restartProfile();
        } catch (error) {
            setProfileError(t("data_move_failed").replace("{e}", String(error)));
        } finally {
            setProfileBusy(false);
        }
    };

    return (
        <div className={`settings-group ${collapsed ? "collapsed" : ""}`}>
            <div className="group-header" onClick={onToggle}>
                <h3 style={{ margin: 0 }}>{t("data_management")}</h3>
                {collapsed ? <ChevronRight size={16} /> : <ChevronDown size={16} />}
            </div>
            {!collapsed && (
                <div className="group-content">
                    <SettingsDataPath label={t("data_path")} path={displayPath(movedProfile?.path ?? dataPath)}>
                        {movedProfile ? (
                            <SettingsDataAction disabled={profileBusy} onClick={async () => {
                                setProfileBusy(true);
                                await restartProfile();
                                setProfileBusy(false);
                            }}>
                                {t("common_retry")}
                            </SettingsDataAction>
                        ) : <>
                            <SettingsDataAction disabled={!dataPath || backupDisabled || backupDialog !== null}
                                onClick={changeDirectory}>
                                {t("change_app")}
                            </SettingsDataAction>
                            <SettingsDataAction disabled={!dataPath || checkingProfile || profileBusy} onClick={() => {
                                setProfileError("");
                                void invoke("open_data_folder").catch(() => setProfileError(t("native_profile_open_failed")));
                            }}>
                                {t("open_folder")}
                            </SettingsDataAction>
                        </>}
                    </SettingsDataPath>
                    {profileBusy && <p className="settings-value" role="status">{t("processing")}</p>}
                    {movedProfile && !profileBusy && !profileError &&
                        <p className="settings-value" role="status">{t("data_move_retry")}</p>}
                    {profileError && <p className="data-error" role="alert">{profileError}</p>}

                    <div className="data-actions-row">
                        <SettingsDataAction icon="export" disabled={backupDisabled}
                            onClick={() => openBackup("export")}
                        >
                            {t("data.export.title")}
                        </SettingsDataAction>
                        <SettingsDataAction icon="import" disabled={backupDisabled}
                            onClick={() => openBackup("import")}
                        >
                            {t("data.import.title")}
                        </SettingsDataAction>
                    </div>
                    {privateSession && <p className="settings-value">{t("native_backup_private")}</p>}
                    {completed && <p className="settings-value" role="status">{t(`data.${completed}.success`)}</p>}
                </div>
            )}

            {backupDialog === "export" && (
                <ExportModal t={t} onClose={() => setBackupDialog(null)} onComplete={() => setCompleted("export")} />
            )}
            {backupDialog === "import" && (
                <ImportModal t={t} onClose={() => setBackupDialog(null)} onComplete={() => setCompleted("import")} />
            )}
        </div>
    );
};

export default DataSettingsGroup;
