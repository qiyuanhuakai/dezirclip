import { SETTINGS_GROUPS, type SettingsGroupId } from "../utils/settingsLayout";

interface SettingsRailProps {
    readonly t: (key: string) => string;
    readonly activeGroup: SettingsGroupId;
    readonly onSelect: (id: SettingsGroupId) => void;
}

/**
 * Left-hand navigation for the wide settings layout. Labels come from the same
 * locale keys the accordion headers use, so no translation is duplicated and
 * `pnpm run i18n:check` still covers them.
 */
const SettingsRail = ({ t, activeGroup, onSelect }: SettingsRailProps) => (
    <nav className="settings-rail" aria-label={t("settings")}>
        {SETTINGS_GROUPS.map(({ id, labelKey }) => {
            const active = id === activeGroup;
            return (
                <button
                    key={id}
                    type="button"
                    className={`settings-rail-item${active ? " is-active" : ""}`}
                    aria-current={active}
                    onClick={() => onSelect(id)}
                >
                    {t(labelKey)}
                </button>
            );
        })}
    </nav>
);

export default SettingsRail;
