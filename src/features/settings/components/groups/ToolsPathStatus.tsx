export interface CliInfo {
    readonly cli_path: string | null;
    readonly cli_version: string;
    readonly skill_path: string | null;
    readonly cli_on_path: boolean;
}

interface ToolsPathStatusProps {
    readonly t: (key: string) => string;
    readonly cliInfo: CliInfo | null;
    readonly cliInfoLoading: boolean;
    readonly pathUpdating: boolean;
    readonly onAddToPath: () => void;
}

export const ToolsPathStatus = ({
    t,
    cliInfo,
    cliInfoLoading,
    pathUpdating,
    onAddToPath,
}: ToolsPathStatusProps) => {
    const badgeText = cliInfoLoading
        ? t("processing")
        : cliInfo?.cli_on_path
            ? t("cli_path_ready_badge")
            : cliInfo?.cli_path
                ? t("cli_path_missing_badge")
                : t("cli_path_not_found_badge");
    const badgeClass = cliInfo?.cli_on_path ? "available" : "unavailable";

    return (
        <div className="tools-path-status-actions">
            <span className={`capture-engine-badge capture-engine-badge--${badgeClass}`}>
                <span className="capture-engine-badge__dot" />
                {badgeText}
            </span>
            {cliInfo?.cli_path && !cliInfo.cli_on_path && (
                <button
                    type="button"
                    className="setting-btn setting-btn--compact"
                    onClick={onAddToPath}
                    disabled={pathUpdating}
                >
                    {pathUpdating ? t("processing") : t("add_to_path")}
                </button>
            )}
        </div>
    );
};
