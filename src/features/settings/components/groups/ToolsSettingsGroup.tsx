import { useState, useEffect, type ComponentType, type ReactNode } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";

interface LabelWithHintProps {
    label: string;
    hint?: string | ReactNode;
    hintKey: string;
}

interface CliInfo {
    cli_path: string | null;
    cli_version: string;
    skill_path: string | null;
}

interface ToolsSettingsGroupProps {
    t: (key: string) => string;
    collapsed: boolean;
    onToggle: () => void;
    LabelWithHint: ComponentType<LabelWithHintProps>;
}

const ToolsSettingsGroup = ({
    t,
    collapsed,
    onToggle,
    LabelWithHint,
}: ToolsSettingsGroupProps) => {
    const [cliInfo, setCliInfo] = useState<CliInfo | null>(null);
    const [appVersion, setAppVersion] = useState("");
    const [platform, setPlatform] = useState("");
    const [buildDate] = useState(() => {
        try {
            return new Date().toISOString().split("T")[0];
        } catch {
            return "";
        }
    });

    useEffect(() => {
        invoke<CliInfo>("get_cli_info")
            .then(setCliInfo)
            .catch(() => {});
    }, []);

    useEffect(() => {
        import("@tauri-apps/api/app").then(({ getVersion }) => {
            getVersion().then(setAppVersion).catch(() => {});
        });
        invoke<{ is_linux: boolean }>("get_platform_info")
            .then((info) => setPlatform(info.is_linux ? "linux" : "windows"))
            .catch(() => setPlatform(navigator.platform.startsWith("Win") ? "windows" : "linux"));
    }, []);

    const handleCopyInstallCommand = () => {
        const cmd = platform === "windows"
            ? "powershell -ExecutionPolicy Bypass -File skills/tiez-c-cli/install.ps1"
            : "bash skills/tiez-c-cli/install.sh";
        navigator.clipboard.writeText(cmd).catch(console.error);
    };

    return (
        <div className={`settings-group ${collapsed ? "collapsed" : ""}`}>
            <div className="group-header" onClick={onToggle}>
                <h3 style={{ margin: 0 }}>{t("tools_settings")}</h3>
                {collapsed ? <ChevronRight size={16} /> : <ChevronDown size={16} />}
            </div>
            {!collapsed && (
                <div className="group-content">
                    <div className="setting-item">
                        <LabelWithHint
                            label={t("tiez_c_cli")}
                            hint={t("tiez_c_cli")}
                            hintKey="tiez_c_cli"
                        />
                    </div>

                    {cliInfo?.cli_path && (
                        <div className="setting-item" style={{ marginLeft: "18px" }}>
                            <div className="item-label-group">
                                <span className="item-label">{t("cli_path")}</span>
                            </div>
                            <span style={{ fontSize: "12px", opacity: 0.7, maxWidth: "200px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                                {cliInfo.cli_path}
                            </span>
                        </div>
                    )}

                    {cliInfo?.cli_version && (
                        <div className="setting-item" style={{ marginLeft: "18px" }}>
                            <div className="item-label-group">
                                <span className="item-label">{t("cli_version")}</span>
                            </div>
                            <span style={{ fontSize: "12px", opacity: 0.7 }}>
                                {cliInfo.cli_version}
                            </span>
                        </div>
                    )}

                    <div className="setting-item" style={{ marginLeft: "18px" }}>
                        <button
                            className="setting-btn setting-btn--compact"
                            onClick={handleCopyInstallCommand}
                        >
                            {t("copy_install_command")}
                        </button>
                    </div>

                    <div className="setting-item">
                        <LabelWithHint
                            label={t("agent_skill")}
                            hint={t("agent_skill")}
                            hintKey="agent_skill"
                        />
                    </div>

                    {cliInfo?.skill_path && (
                        <div className="setting-item" style={{ marginLeft: "18px" }}>
                            <div className="item-label-group">
                                <span className="item-label">{t("skill_path")}</span>
                            </div>
                            <span style={{ fontSize: "12px", opacity: 0.7, maxWidth: "200px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                                {cliInfo.skill_path}
                            </span>
                        </div>
                    )}

                    <div className="setting-item">
                        <LabelWithHint
                            label={t("build_info")}
                            hint={t("build_info")}
                            hintKey="build_info"
                        />
                    </div>

                    <div className="setting-item" style={{ marginLeft: "18px" }}>
                        <div className="item-label-group">
                            <span className="item-label">{t("app_version")}</span>
                        </div>
                        <span style={{ fontSize: "12px", opacity: 0.7 }}>
                            {appVersion || "-"}
                        </span>
                    </div>

                    <div className="setting-item" style={{ marginLeft: "18px" }}>
                        <div className="item-label-group">
                            <span className="item-label">{t("platform_label")}</span>
                        </div>
                        <span style={{ fontSize: "12px", opacity: 0.7 }}>
                            {platform === "linux" ? t("platform_linux") : t("platform_windows")}
                        </span>
                    </div>

                    <div className="setting-item" style={{ marginLeft: "18px" }}>
                        <div className="item-label-group">
                            <span className="item-label">{t("build_date")}</span>
                        </div>
                        <span style={{ fontSize: "12px", opacity: 0.7 }}>
                            {buildDate || "-"}
                        </span>
                    </div>
                </div>
            )}
        </div>
    );
};

export default ToolsSettingsGroup;
