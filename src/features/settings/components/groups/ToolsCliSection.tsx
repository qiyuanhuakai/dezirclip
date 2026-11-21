import { useCallback, useEffect, useState } from "react";
import { CLISettings } from "@deziro/ui-react/components/CLISettings";
import { invoke } from "../../../../shared/lib/desktopInvoke";
import { displayPath } from "../../utils/displayPath";

interface CliInfo {
    readonly cli_path: string | null;
    readonly cli_version: string;
    readonly skill_path: string | null;
    readonly theme_skill_path: string | null;
    readonly cli_on_path: boolean;
    readonly cli_registered_for_user: boolean;
    readonly path_cli_path: string | null;
    readonly path_cli_version: string | null;
    readonly cli_path_conflict: boolean;
}

interface CliPathResult {
    readonly path_entry: string;
    readonly already_linked: boolean;
    readonly requires_new_terminal: boolean;
}

let cachedCliInfo: CliInfo | null = null;
let pendingCliInfoRequest: Promise<CliInfo> | null = null;

const loadCliInfo = (refresh: boolean): Promise<CliInfo> => {
    if (pendingCliInfoRequest) return pendingCliInfoRequest;
    if (!refresh && cachedCliInfo) return Promise.resolve(cachedCliInfo);
    pendingCliInfoRequest = invoke<CliInfo>("get_cli_info").then((info) => {
        cachedCliInfo = info;
        return info;
    }).finally(() => { pendingCliInfoRequest = null; });
    return pendingCliInfoRequest;
};

export default function ToolsCliSection({ t }: { readonly t: (key: string) => string }) {
    const [cliInfo, setCliInfo] = useState<CliInfo | null>(cachedCliInfo);
    const [loading, setLoading] = useState(cachedCliInfo === null);
    const [loadFailed, setLoadFailed] = useState(false);
    const [pathUpdating, setPathUpdating] = useState(false);
    const [notice, setNotice] = useState("");

    const refreshCliInfo = useCallback(async (force = false) => {
        setLoading(true);
        setLoadFailed(false);
        try {
            setCliInfo(await loadCliInfo(force));
        } catch {
            setLoadFailed(true);
        } finally {
            setLoading(false);
        }
    }, []);

    useEffect(() => { void refreshCliInfo(); }, [refreshCliInfo]);

    const addToPath = async () => {
        setPathUpdating(true);
        setNotice("");
        try {
            const result = await invoke<CliPathResult>("add_cli_to_path");
            const key = result.already_linked ? "cli_path_already_linked"
                : result.requires_new_terminal ? "cli_path_added_restart" : "cli_path_added";
            setNotice(`${t(key)} ${displayPath(result.path_entry)}`);
            await refreshCliInfo(true);
        } catch (error) {
            setNotice(`${t("cli_path_add_failed")} ${error instanceof Error ? error.message : String(error)}`);
        } finally {
            setPathUpdating(false);
        }
    };

    const openGuide = async (skill: string) => {
        try {
            await invoke("open_agent_skill_folder", { skill });
        } catch (error) {
            setNotice(`${t("open_skill_folder_failed")} ${error instanceof Error ? error.message : String(error)}`);
        }
    };

    const statusKey = loading ? "processing" : loadFailed ? "cli_info_load_failed"
        : cliInfo?.cli_path_conflict ? "cli_path_other_copy_badge"
        : cliInfo?.cli_on_path ? "cli_path_ready_badge"
        : cliInfo?.cli_registered_for_user ? "cli_path_registered_badge"
        : cliInfo?.cli_path ? "cli_path_missing_badge" : "cli_path_not_found_badge";
    const needsPath = cliInfo?.cli_path && !cliInfo.cli_on_path && !cliInfo.cli_registered_for_user;
    const action = loadFailed
        ? { label: t("common_retry"), run: () => void refreshCliInfo(true) }
        : needsPath ? { label: t(pathUpdating ? "processing" : "add_to_path"), disabled: pathUpdating || loading, run: () => void addToPath() }
        : undefined;
    const fields = [];
    if (cliInfo?.cli_path) fields.push({ label: t("cli_path"), value: displayPath(cliInfo.cli_path), path: true });
    if (cliInfo?.cli_version) fields.push({ label: t("cli_version"), value: cliInfo.cli_version });
    const conflict = [];
    if (cliInfo?.cli_path_conflict && cliInfo.path_cli_path) {
        conflict.push({ label: t("cli_path_active_path"), value: displayPath(cliInfo.path_cli_path), path: true });
        if (cliInfo.path_cli_version) conflict.push({ label: t("cli_path_active_version"), value: cliInfo.path_cli_version });
    }
    return <CLISettings title={t("dzc_cli")} description={t("dzc_cli_hint")} helpLabel={t("settings_help")}
        fields={fields} conflict={conflict} notice={notice}
        status={{ label: t("cli_path_status"), value: t(statusKey), action, available: !loadFailed && Boolean(cliInfo?.cli_on_path && !cliInfo.cli_path_conflict) }}
        guidesTitle={t("agent_skill")} guidesDescription={t("agent_skill_hint")} guideOpenLabel={t("open_skill_folder")}
        guides={[
            { id: "dzc-cli", label: "skill_cli", path: cliInfo?.skill_path },
            { id: "dezirclip-theme", label: "skill_theme", path: cliInfo?.theme_skill_path },
        ].map(guide => ({ label: t(guide.label), path: displayPath(guide.path ?? ""), fallback: guide.id,
            disabled: pathUpdating, onOpen: () => void openGuide(guide.id) }))}
    />;
}
