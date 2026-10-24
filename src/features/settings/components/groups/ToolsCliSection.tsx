import { useCallback, useEffect, useState, type ComponentType, type ReactNode } from "react";
import { FolderOpen } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { ToolsPathStatus, type CliInfo } from "./ToolsPathStatus";
import { displayPath } from "../../utils/displayPath";

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
    const [pathUpdating, setPathUpdating] = useState(false);
    const [pathStatusKey, setPathStatusKey] = useState<string | null>(null);
    const [pathStatusDetail, setPathStatusDetail] = useState("");

    const refreshCliInfo = useCallback((forceRefresh = false) => {
        setCliInfoLoading(true);
        loadCliInfo(forceRefresh)
            .then(setCliInfo)
            .catch(() => setCliInfo(null))
            .finally(() => setCliInfoLoading(false));
    }, []);

    useEffect(() => {
        refreshCliInfo();
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

    const handleOpenSkillFolder = async (skill: string) => {
        try {
            await invoke("open_agent_skill_folder", { skill });
        } catch (e: unknown) {
            setPathStatusKey("open_skill_folder_failed");
            setPathStatusDetail(e instanceof Error ? e.message : String(e));
        }
    };

    const pathStatusText = () => {
        switch (pathStatusKey) {
            case "cli_path_added":
            case "cli_path_added_restart":
            case "cli_path_already_linked":
            case "cli_path_add_failed":
            case "open_skill_folder_failed":
                return t(pathStatusKey);
            default:
                return "";
        }
    };

    return (
        <>
            <div className="setting-item settings-section-title">
                <LabelWithHint label={t("dzc_cli")} hintKey="dzc_cli" />
            </div>
            {cliInfo?.cli_path && (
                <div className="setting-item setting-item--detail">
                    <div className="item-label-group"><span className="item-label">{t("cli_path")}</span></div>
                    <span className="tools-path-text" title={displayPath(cliInfo.cli_path)}>{displayPath(cliInfo.cli_path)}</span>
                </div>
            )}
            {cliInfo?.cli_version && (
                <div className="setting-item setting-item--detail">
                    <div className="item-label-group"><span className="item-label">{t("cli_version")}</span></div>
                    <span style={{ fontSize: "12px", opacity: 0.7 }}>{cliInfo.cli_version}</span>
                </div>
            )}
            <div className="setting-item setting-item--detail tools-action-row">
                <div className="item-label-group"><span className="item-label">{t("cli_path_status")}</span></div>
                <ToolsPathStatus t={t} cliInfo={cliInfo} cliInfoLoading={cliInfoLoading} pathUpdating={pathUpdating} onAddToPath={handleAddCliToPath} />
            </div>
            {cliInfo?.cli_path_conflict && (
                <>
                    <div className="setting-item setting-item--detail">
                        <div className="item-label-group"><span className="item-label">{t("cli_path_active_path")}</span></div>
                        <span className="tools-path-text" title={cliInfo.path_cli_path ? displayPath(cliInfo.path_cli_path) : undefined}>{cliInfo.path_cli_path ? displayPath(cliInfo.path_cli_path) : ""}</span>
                    </div>
                    {cliInfo.path_cli_version && (
                        <div className="setting-item setting-item--detail">
                            <div className="item-label-group"><span className="item-label">{t("cli_path_active_version")}</span></div>
                            <span className="tools-note">{cliInfo.path_cli_version}</span>
                        </div>
                    )}
                </>
            )}
            {pathStatusKey && (
                <div className="setting-item setting-item--detail tools-note-item">
                    <span className="tools-note">{pathStatusText()}{pathStatusDetail ? ` ${pathStatusDetail}` : ""}</span>
                </div>
            )}
            <div className="setting-item settings-section-title">
                <LabelWithHint label={t("agent_skill")} hintKey="agent_skill" />
            </div>
            {[
                { id: "dzc-cli", label: "skill_cli", path: cliInfo?.skill_path },
                { id: "dezirclip-theme", label: "skill_theme", path: cliInfo?.theme_skill_path },
            ].map(skill => (
                <div key={skill.id} className="setting-item setting-item--detail tools-action-row">
                    <div className="item-label-group"><span className="item-label">{t(skill.label)}</span>
                        <span className="tools-path-text" title={skill.path ? displayPath(skill.path) : undefined}>{skill.path ? displayPath(skill.path) : skill.id}</span>
                    </div>
                    <button type="button" className="setting-btn setting-btn--compact" disabled={!skill.path}
                        onClick={() => void handleOpenSkillFolder(skill.id)} title={t("open_skill_folder")}
                        aria-label={`${t("open_skill_folder")}: ${t(skill.label)}`}>
                        <FolderOpen size={14} />
                    </button>
                </div>
            ))}
        </>
    );
};

export default ToolsCliSection;
