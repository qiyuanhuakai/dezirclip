import { useMemo, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { SettingsField } from "@deziro/ui-react/components/SettingsField";
import ThemedSelect, { type ThemedSelectOption } from "../ThemedSelect";
import type { DefaultAppsMap, InstalledAppOption } from "../../../app/types";
import { getClipboardTypeName } from "../../../../shared/lib/clipboardTypeName";

const SYSTEM_DEFAULT_VALUE = "__system_default__";
const APP_TYPES = ["text", "rich_text", "image", "video", "code", "url"] as const;

interface DefaultAppsSettingsGroupProps {
    t: (key: string) => string;
    collapsed: boolean;
    onToggle: () => void;
    installedApps: InstalledAppOption[];
    appSettings: Record<string, string>;
    defaultApps: DefaultAppsMap;
    saveAppSetting: (key: string, val: string) => void;
}

const stripExe = (path: string) => {
    const filename = path.split(/[/\\]/).pop() || path;
    return filename.replace(/\.exe$/i, "");
};

const DefaultAppsSettingsGroup = ({
    t,
    collapsed,
    onToggle,
    installedApps,
    appSettings,
    defaultApps,
    saveAppSetting
}: DefaultAppsSettingsGroupProps) => {
    const [saving, setSaving] = useState(false);

    const optionsByType = useMemo(() => {
        const installedByPath = new Map<string, string>();
        const nameCounts = new Map<string, number>();
        for (const { value, label } of installedApps) {
            if (!value || installedByPath.has(value)) continue;
            const name = label || stripExe(value);
            installedByPath.set(value, name);
            const folded = name.toLowerCase();
            nameCounts.set(folded, (nameCounts.get(folded) ?? 0) + 1);
        }
        const appOptions = Array.from(installedByPath, ([value, label]) => ({
            value,
            label: (nameCounts.get(label.toLowerCase()) ?? 0) > 1 ? `${label} (${value})` : label,
        }));
        const map: Record<string, ThemedSelectOption[]> = {};
        for (const type of APP_TYPES) {
            const sysValue = `${SYSTEM_DEFAULT_VALUE}::${type}`;
            const inherited = type === "code" ? appSettings["app.text"] : "";
            const name = inherited
                ? installedByPath.get(inherited) || stripExe(inherited)
                : stripExe(defaultApps[type] ?? "");
            const fallbackLabel = t(inherited ? "default_app_follow_text" : "system_default");
            const opts: ThemedSelectOption[] = [
                { value: sysValue, label: name ? `${fallbackLabel} (${name})` : fallbackLabel },
                ...appOptions,
            ];
            const saved = appSettings[`app.${type}`];
            if (saved && !installedByPath.has(saved)) opts.push({ value: saved, label: stripExe(saved) });
            map[type] = opts;
        }
        return map;
    }, [installedApps, defaultApps, appSettings, t]);

    const currentValue = (type: string) => {
        const path = appSettings[`app.${type}`];
        if (path) return path;
        return `${SYSTEM_DEFAULT_VALUE}::${type}`;
    };

    return (
        <div className={`settings-group ${collapsed ? 'collapsed' : ''}`}>
            <div className="group-header" onClick={onToggle}>
                <h3 style={{ margin: 0 }}>{t('default_apps')}</h3>
                {collapsed ? <ChevronRight size={16} /> : <ChevronDown size={16} />}
            </div>
            {!collapsed && (
                <div className="group-content">
                    {APP_TYPES.map((type, idx, arr) => {
                        const systemDefault = defaultApps[type];
                        const userPath = appSettings[`app.${type}`];
                        const value = currentValue(type);
                        const options = optionsByType[type];
                        return (
                            <SettingsField key={type} stacked last={idx === arr.length - 1}>
                                <div className="item-label-group" style={{ marginBottom: '8px' }}>
                                    <span className="item-label" style={{ textTransform: 'uppercase', fontSize: '11px', opacity: 0.8 }}>
                                        {getClipboardTypeName(type, t)}
                                    </span>
                                </div>
                                <ThemedSelect
                                    ariaLabel={getClipboardTypeName(type, t)}
                                    isDisabled={saving}
                                    options={options}
                                    value={value}
                                    width="100%"
                                    placeholder={!systemDefault ? t('not_configured') : undefined}
                                    searchable
                                    noOptionsMessage={t('no_matching_apps') || '无匹配应用'}
                                    onChange={async (val) => {
                                        setSaving(true);
                                        try {
                                            await saveAppSetting(type, val === `${SYSTEM_DEFAULT_VALUE}::${type}` ? '' : val);
                                        } finally {
                                            setSaving(false);
                                        }
                                    }}
                                />
                                {userPath && (
                                    <span className="hint" style={{ fontSize: '10px', color: 'var(--text-secondary)', marginTop: '2px', wordBreak: 'break-all' }}>
                                        {stripExe(userPath)}
                                    </span>
                                )}
                            </SettingsField>
                        );
                    })}
                </div>
            )}
        </div>
    );
};

export default DefaultAppsSettingsGroup;
