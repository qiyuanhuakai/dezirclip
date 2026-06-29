import { useCallback, useEffect, useState, type ComponentType, type ReactNode } from "react";
import { FolderOpen } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { ToolsPathStatus, type CliInfo } from "./ToolsPathStatus";

interface LabelWithHintProps {
    readonly label: string;
    readonly hint?: string | ReactNode;
    readonly hintKey: string;
}

interface CliPathResult {
    readonly installed_path: string;
    readonly path_entry: string;
    readonly already_linked: boolean;
    readonly requires_new_terminal: boolean;
}

interface ToolsCliSectionProps {
    readonly t: (key: string) => string;
    readonly LabelWithHint: ComponentType<LabelWithHintProps>;
}

let cachedCliInfo: CliInfo | null = null;
let pendingCliInfoRequest: Promise<CliInfo> | null = null;

const loadCliInfo = (forceRefresh: boolean): Promise<CliInfo> => {
    if (!forceRefresh && cachedCliInfo) return Promise.resolve(cachedCliInfo);
    if (!forceRefresh && pendingCliInfoRequest) return pendingCliInfoRequest;
    const request = invoke<CliInfo>("get_cli_info").then((info) => {
        cachedCliInfo = info;
        return info;
    });
    pendingCliInfoRequest = request.finally(() => {
        pendingCliInfoRequest = null;
    });
    return pendingCliInfoRequest;
};

const ToolsCliSection = ({ t, LabelWithHint }: ToolsCliSectionProps) => {
    const [cliInfo, setCliInfo] = useState<CliInfo | null>(cachedCliInfo);
    const [cliInfoLoading, setCliInfoLoading] = useState(false);
    const [appVersion, setAppVersion] = useState("");
    const [platformKey, setPlatformKey] = useState("");
    const [pathUpdating, setPathUpdating] = useState(false);
    const [pathStatusKey, setPathStatusKey] = useState<string | null>(null);
    const [pathStatusDetail, setPathStatusDetail] = useState("");
    const [buildDate] = useState(() => new Date().toISOString().slice(0, 10));

    const refreshCliInfo = useCallback((forceRefresh = false) => {
        setCliInfoLoading(true);
        loadCliInfo(forceRefresh)
            .then(setCliInfo)
            .catch(() => setCliInfo(null))
            .finally(() => setCliInfoLoading(false));
    }, []);

    useEffect(() => {
        refreshCliInfo();
        import("@tauri-apps/api/app").then(({ getVersion }) => {
            getVersion().then(setAppVersion).catch(() => setAppVersion(""));
        });
        invoke<{ is_linux: boolean }>("get_platform_info")
            .then((info) => setPlatformKey(info.is_linux ? "platform_linux" : "platform_windows"))
            .catch(() => setPlatformKey(""));
    }, [refreshCliInfo]);

    const handleAddCliToPath = async () => {
        setPathUpdating(true);
        setPathStatusKey(null);
        setPathStatusDetail("");
        try {
            const result = await invoke<CliPathResult>("add_cli_to_path");
            refreshCliInfo(true);
            setPathStatusKey(result.already_linked
                ? "cli_path_already_linked"
                : result.requires_new_terminal
                    ? "cli_path_added_restart"
                    : "cli_path_added");
            setPathStatusDetail(result.path_entry);
        } catch (e: unknown) {
            setPathStatusKey("cli_path_add_failed");
            setPathStatusDetail(e instanceof Error ? e.message : String(e));
        } finally {
            setPathUpdating(false);
        }
    };

    const handleOpenInstallFolder = async () => {
        try {
            await invoke("open_install_folder");
        } catch (e: unknown) {
            setPathStatusKey("open_install_folder_failed");
            setPathStatusDetail(e instanceof Error ? e.message : String(e));
        }
    };

    const pathStatusText = () => {
        switch (pathStatusKey) {
            case "cli_path_added":
            case "cli_path_added_restart":
            case "cli_path_already_linked":
            case "cli_path_add_failed":
            case "open_install_folder_failed":
                return t(pathStatusKey);
            default:
                return "";
        }
    };

    return (
        <>
            <div className="setting-item">
                <LabelWithHint label={t("dzc_cli")} hint={t("dzc_cli")} hintKey="dzc_cli" />
            </div>
            {cliInfo?.cli_path && (
                <div className="setting-item" style={{ marginLeft: "18px" }}>
                    <div className="item-label-group"><span className="item-label">{t("cli_path")}</span></div>
                    <span style={{ fontSize: "12px", opacity: 0.7, maxWidth: "200px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{cliInfo.cli_path}</span>
                </div>
            )}
            {cliInfo?.cli_version && (
                <div className="setting-item" style={{ marginLeft: "18px" }}>
                    <div className="item-label-group"><span className="item-label">{t("cli_version")}</span></div>
                    <span style={{ fontSize: "12px", opacity: 0.7 }}>{cliInfo.cli_version}</span>
                </div>
            )}
            <div className="setting-item tools-action-row" style={{ marginLeft: "18px" }}>
                <div className="item-label-group"><span className="item-label">{t("cli_path_status")}</span></div>
                <ToolsPathStatus t={t} cliInfo={cliInfo} cliInfoLoading={cliInfoLoading} pathUpdating={pathUpdating} onAddToPath={handleAddCliToPath} />
            </div>
            {pathStatusKey && (
                <div className="setting-item tools-note-item" style={{ marginLeft: "18px" }}>
                    <span className="tools-note">{pathStatusText()}{pathStatusDetail ? ` ${pathStatusDetail}` : ""}</span>
                </div>
            )}
            <div className="setting-item column tools-skill-card">
                <LabelWithHint label={t("agent_skill")} hint={t("agent_skill")} hintKey="agent_skill" />
                <div className="tools-skill-method">{t("skill_acquire_method")}</div>
                <button type="button" className="tools-skill-command tools-skill-command--button" onClick={handleOpenInstallFolder} title={t("open_install_folder")}>
                    <FolderOpen size={14} />
                    <span>{t("open_install_folder")}</span>
                </button>
            </div>
            {cliInfo?.skill_path && (
                <div className="setting-item" style={{ marginLeft: "18px" }}>
                    <div className="item-label-group"><span className="item-label">{t("skill_path")}</span></div>
                    <span className="tools-path-text">{cliInfo.skill_path}</span>
                </div>
            )}
            <div className="setting-item">
                <LabelWithHint label={t("build_info")} hint={t("build_info")} hintKey="build_info" />
            </div>
            {[["app_version", appVersion || "-"], ["platform_label", platformKey ? t(platformKey) : "-"], ["build_date", buildDate || "-"]].map(([labelKey, value]) => (
                <div key={labelKey} className="setting-item" style={{ marginLeft: "18px" }}>
                    <div className="item-label-group"><span className="item-label">{t(labelKey)}</span></div>
                    <span style={{ fontSize: "12px", opacity: 0.7 }}>{value}</span>
                </div>
            ))}
        </>
    );
};

export default ToolsCliSection;
