export interface SettingsRailProps<Id extends string> {
    readonly t: (key: string) => string;
    readonly groups: readonly { id: Id; labelKey: string }[];
    readonly activeGroup: Id;
    readonly onSelect: (id: Id) => void;
}

/**
 * Left-hand navigation for the wide settings layout. Labels come from the same
 * locale keys the accordion headers use, so no translation is duplicated and
 * `pnpm run i18n:check` still covers them.
 */
const SettingsRail = <Id extends string,>({ t, groups, activeGroup, onSelect }: SettingsRailProps<Id>) => (
    <nav className="settings-rail" aria-label={t("settings")}>
        {groups.map(({ id, labelKey }) => {
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
