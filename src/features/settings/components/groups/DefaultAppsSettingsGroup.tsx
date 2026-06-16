import { ChevronDown, ChevronRight } from "lucide-react";
import ThemedSelect from "../ThemedSelect";
import type { DefaultAppsMap, InstalledAppOption } from "../../../app/types";
import { getClipboardTypeName } from "../../../../shared/lib/clipboardTypeName";

const SYSTEM_DEFAULT_VALUE = "__system_default__";

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
    const APP_TYPES = ['text', 'image', 'video', 'code', 'url'] as const;

    const buildOptions = (type: string) => {
        const seen = new Set<string>();
        const opts: { value: string; label: string }[] = [];
        if (defaultApps[type]) {
            const sysValue = `${SYSTEM_DEFAULT_VALUE}::${type}`;
            opts.push({
                value: sysValue,
                label: `${t('system_default')} (${stripExe(defaultApps[type])})`
            });
            seen.add(sysValue);
        }
        for (const app of installedApps) {
            if (!seen.has(app.value)) {
                seen.add(app.value);
                opts.push({ value: app.value, label: app.label });
            }
        }
        return opts;
    };

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
                        return (
                            <div key={type} className={`setting-item column ${idx === arr.length - 1 ? 'no-border' : ''}`}>
                                <div className="item-label-group" style={{ marginBottom: '8px' }}>
                                    <span className="item-label" style={{ textTransform: 'uppercase', fontSize: '11px', opacity: 0.8 }}>
                                        {getClipboardTypeName(type, t)}
                                    </span>
                                </div>
                                <ThemedSelect
                                    options={buildOptions(type)}
                                    value={value}
                                    width="100%"
                                    placeholder={!systemDefault ? t('not_configured') : undefined}
                                    onChange={(val) => {
                                        if (val.startsWith(SYSTEM_DEFAULT_VALUE)) {
                                            saveAppSetting(type, '');
                                        } else {
                                            saveAppSetting(type, val);
                                        }
                                    }}
                                />
                                {userPath && (
                                    <span className="hint" style={{ fontSize: '10px', color: 'var(--text-secondary)', marginTop: '2px', wordBreak: 'break-all' }}>
                                        {stripExe(userPath)}
                                    </span>
                                )}
                            </div>
                        );
                    })}
                </div>
            )}
        </div>
    );
};

export default DefaultAppsSettingsGroup;
