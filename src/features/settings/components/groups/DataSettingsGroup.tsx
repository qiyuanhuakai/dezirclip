import { useState } from "react";
import { open, save, ask, message } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { ChevronDown, ChevronRight, Download, Upload } from "lucide-react";

interface DataSettingsGroupProps {
    t: (key: string) => string;
    collapsed: boolean;
    onToggle: () => void;
    dataPath: string;
}

interface ExportModalProps {
    t: (key: string) => string;
    onClose: () => void;
}

interface ImportModalProps {
    t: (key: string) => string;
    onClose: () => void;
}

const ExportModal = ({ t, onClose }: ExportModalProps) => {
    const [format, setFormat] = useState<"json" | "encrypted">("json");
    const [passphrase, setPassphrase] = useState("");
    const [passphraseConfirm, setPassphraseConfirm] = useState("");
    const [filePath, setFilePath] = useState("");
    const [passphraseError, setPassphraseError] = useState("");
    const [passphraseMismatchError, setPassphraseMismatchError] = useState("");
    const [exporting, setExporting] = useState(false);

    const handleBrowse = async () => {
        const path = await save({
            defaultPath: format === "json" ? "clipboard-export.json" : "clipboard-export.dzc",
            filters: format === "json"
                ? [{ name: "JSON", extensions: ["json"] }]
                : [{ name: "Encrypted Backup", extensions: ["dzc"] }],
        });
        if (path) setFilePath(path);
    };

    const validate = (): boolean => {
        let valid = true;
        setPassphraseError("");
        setPassphraseMismatchError("");

        if (format === "encrypted") {
            if (passphrase.length < 12) {
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
        setExporting(true);
        try {
            await invoke("export_to_file", {
                path: filePath,
                format,
                passphrase: format === "encrypted" ? passphrase : undefined,
            });
            onClose();
        } catch (e: unknown) {
            console.error(e);
        } finally {
            setExporting(false);
        }
    };

    return (
        <div className="data-modal-overlay" onClick={onClose}>
            <div className="data-modal-content" onClick={(e) => e.stopPropagation()}>
                <div className="data-modal-title">{t("data.export.title")}</div>

                <div className="data-form-group">
                    <label className="data-label">{t("data.export.choose_format")}</label>
                    <div className="data-radio-group">
                        <label className="data-radio-item">
                            <input
                                type="radio"
                                name="export-format"
                                value="json"
                                checked={format === "json"}
                                onChange={() => setFormat("json")}
                            />
                            <span>{t("data.export.json")}</span>
                        </label>
                        <label className="data-radio-item">
                            <input
                                type="radio"
                                name="export-format"
                                value="encrypted"
                                checked={format === "encrypted"}
                                onChange={() => setFormat("encrypted")}
                            />
                            <span>{t("data.export.encrypted")}</span>
                        </label>
                    </div>
                </div>

                {format === "encrypted" && (
                    <div className="data-form-group">
                        <label className="data-label">{t("data.export.passphrase_prompt")}</label>
                        <input
                            type="password"
                            className="data-input"
                            value={passphrase}
                            onChange={(e) => setPassphrase(e.target.value)}
                            placeholder="••••••••••••"
                        />
                        {passphraseError && (
                            <span className="data-error">{passphraseError}</span>
                        )}
                        <label className="data-label">{t("data.export.confirm_passphrase")}</label>
                        <input
                            type="password"
                            className="data-input"
                            value={passphraseConfirm}
                            onChange={(e) => setPassphraseConfirm(e.target.value)}
                            placeholder="••••••••••••"
                        />
                        {passphraseMismatchError && (
                            <span className="data-error">{passphraseMismatchError}</span>
                        )}
                    </div>
                )}

                <div className="data-form-group">
                    <label className="data-label">{t("data_path")}</label>
                    <div className="data-path-row">
                        <input
                            type="text"
                            className="data-input data-input-path"
                            value={filePath}
                            readOnly
                        />
                        <button
                            className="btn-icon btn-icon-export-browse"
                            onClick={handleBrowse}
                        >
                            {t("browse_file")}
                        </button>
                    </div>
                </div>

                <div className="data-modal-actions">
                    <button className="data-btn data-btn-cancel" onClick={onClose}>
                        {t("cancel")}
                    </button>
                    <button
                        className="data-btn data-btn-primary"
                        onClick={handleExport}
                        disabled={exporting || !filePath}
                    >
                        {exporting ? t("processing") : t("data.export.title")}
                    </button>
                </div>
            </div>
        </div>
    );
};

const ImportModal = ({ t, onClose }: ImportModalProps) => {
    const [mode, setMode] = useState<"merge" | "replace">("merge");
    const [filePath, setFilePath] = useState("");
    const [passphrase, setPassphrase] = useState("");
    const [importing, setImporting] = useState(false);
    const [previewEntries, setPreviewEntries] = useState<Array<{ content: string }>>([]);
    const [showPreview, setShowPreview] = useState(false);

    const handleBrowse = async () => {
        const selected = await open({
            multiple: false,
            filters: [{ name: "Backup", extensions: ["json", "dzc", "tiez"] }],
        });
        if (selected) {
            const path = selected as string;
            setFilePath(path);
            setPreviewEntries([]);
            setShowPreview(false);

            if (path.endsWith(".dzc") || path.endsWith(".tiez")) {
                setPassphrase("");
            } else {
                try {
                    const data = await invoke<Array<{ content: string }>>("export_to_file", {
                        path,
                        format: "json",
                    });
                    setPreviewEntries(data.slice(0, 10));
                    setShowPreview(true);
                } catch {
                    setPreviewEntries([]);
                    setShowPreview(false);
                }
            }
        }
    };

    const handleImport = async () => {
        if (!filePath) return;
        setImporting(true);
        try {
            await invoke("import_from_file", {
                path: filePath,
                mode,
                passphrase: filePath.endsWith(".dzc") || filePath.endsWith(".tiez") ? passphrase : undefined,
            });
            onClose();
        } catch (e: unknown) {
            console.error(e);
        } finally {
            setImporting(false);
        }
    };

    return (
        <div className="data-modal-overlay" onClick={onClose}>
            <div className="data-modal-content" onClick={(e) => e.stopPropagation()}>
                <div className="data-modal-title">{t("data.import.title")}</div>

                <div className="data-form-group">
                    <label className="data-label">{t("data.import.mode")}</label>
                    <div className="data-radio-group">
                        <label className="data-radio-item">
                            <input
                                type="radio"
                                name="import-mode"
                                value="merge"
                                checked={mode === "merge"}
                                onChange={() => setMode("merge")}
                            />
                            <span>{t("data.import.merge")}</span>
                        </label>
                        <label className="data-radio-item">
                            <input
                                type="radio"
                                name="import-mode"
                                value="replace"
                                checked={mode === "replace"}
                                onChange={() => setMode("replace")}
                            />
                            <span>{t("data.import.replace")}</span>
                        </label>
                    </div>
                </div>

                <div className="data-form-group">
                    <label className="data-label">{t("data_path")}</label>
                    <div className="data-path-row">
                        <input
                            type="text"
                            className="data-input data-input-path"
                            value={filePath}
                            readOnly
                        />
                        <button
                            className="btn-icon btn-icon-export-browse"
                            onClick={handleBrowse}
                        >
                            {t("browse_file")}
                        </button>
                    </div>
                </div>

                {(filePath.endsWith(".dzc") || filePath.endsWith(".tiez")) && (
                    <div className="data-form-group">
                        <label className="data-label">{t("data.import.passphrase_prompt")}</label>
                        <input
                            type="password"
                            className="data-input"
                            value={passphrase}
                            onChange={(e) => setPassphrase(e.target.value)}
                            placeholder="••••••••••••"
                        />
                    </div>
                )}

                {showPreview && previewEntries.length > 0 && (
                    <div className="data-form-group">
                        <label className="data-label">{t("data.import.preview")}</label>
                        <div className="data-preview-list">
                            {previewEntries.map((entry, i) => (
                                <div key={i} className="data-preview-item">
                                    {entry.content}
                                </div>
                            ))}
                        </div>
                    </div>
                )}

                <div className="data-modal-actions">
                    <button className="data-btn data-btn-cancel" onClick={onClose}>
                        {t("cancel")}
                    </button>
                    <button
                        className="data-btn data-btn-primary"
                        onClick={handleImport}
                        disabled={importing || !filePath}
                    >
                        {importing ? t("processing") : t("data.import.title")}
                    </button>
                </div>
            </div>
        </div>
    );
};

const DataSettingsGroup = ({ t, collapsed, onToggle, dataPath }: DataSettingsGroupProps) => {
    const [showExportModal, setShowExportModal] = useState(false);
    const [showImportModal, setShowImportModal] = useState(false);

    return (
        <div className={`settings-group ${collapsed ? "collapsed" : ""}`}>
            <div className="group-header" onClick={onToggle}>
                <h3 style={{ margin: 0 }}>{t("data_management")}</h3>
                {collapsed ? <ChevronRight size={16} /> : <ChevronDown size={16} />}
            </div>
            {!collapsed && (
                <div className="group-content">
                    <div className="setting-item column no-border">
                        <div className="data-path-header">
                            <span className="item-label data-path-label">{t("data_path")}</span>
                            <div className="data-path-actions">
                                <button
                                    className="btn-icon btn-icon-data-action"
                                    onClick={() => {
                                        open({
                                            directory: true,
                                            multiple: false,
                                            title: t("change_data_path"),
                                        }).then(async (selected) => {
                                            if (selected) {
                                                const newPath = selected as string;
                                                const confirm = await ask(
                                                    t("data_move_confirm").replace("{path}", newPath),
                                                    { title: t("change_data_path"), kind: "warning", okLabel: t("confirm"), cancelLabel: t("cancel") }
                                                );

                                                if (confirm) {
                                                    try {
                                                        await invoke("set_data_path", { newPath });

                                                        await message(
                                                            t("data_move_success"),
                                                            { title: t("notice"), kind: "info" }
                                                        );

                                                        await invoke("relaunch");
                                                    } catch (e: unknown) {
                                                        console.error(e);
                                                        const errorMsg = e instanceof Error ? e.message : String(e);
                                                        await message(
                                                            t("data_move_failed").replace("{e}", errorMsg),
                                                            { title: t("error"), kind: "error" }
                                                        );
                                                    }
                                                }
                                            }
                                        });
                                    }}
                                >
                                    {t("change_app")}
                                </button>
                                <button
                                    className="btn-icon btn-icon-data-action"
                                    onClick={() => invoke("open_data_folder").catch(console.error)}
                                    title={t("open_folder") || "Open"}
                                >
                                    {t("open_folder")}
                                </button>
                            </div>
                        </div>
                        <div className="data-panel">
                            {dataPath}
                        </div>
                    </div>

                    <div className="data-actions-row">
                        <button
                            className="btn-icon btn-icon-data-action"
                            onClick={() => setShowExportModal(true)}
                        >
                            <Download size={14} />
                            <span>{t("data.export.title")}</span>
                        </button>
                        <button
                            className="btn-icon btn-icon-data-action"
                            onClick={() => setShowImportModal(true)}
                        >
                            <Upload size={14} />
                            <span>{t("data.import.title")}</span>
                        </button>
                    </div>
                </div>
            )}

            {showExportModal && (
                <ExportModal t={t} onClose={() => setShowExportModal(false)} />
            )}
            {showImportModal && (
                <ImportModal t={t} onClose={() => setShowImportModal(false)} />
            )}
        </div>
    );
};

export default DataSettingsGroup;
