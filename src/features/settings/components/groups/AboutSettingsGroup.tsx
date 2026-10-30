import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronDown, ChevronRight } from "lucide-react";
import UpdateSettingsSection from "./UpdateSettingsSection";
import SettingsDisclosure from "../SettingsDisclosure";

interface BuildInfo {
    readonly component_version: string;
    readonly build_id: string | null;
    readonly source_revision: string | null;
    readonly source_state: string;
    readonly target: string;
    readonly protocol_version: { readonly major: number; readonly minor: number };
    readonly database_schema_version: number;
    readonly linked_components: readonly { readonly component: string; readonly version: string }[];
}

const BuildInfoSection = ({ t }: { readonly t: (key: string) => string }) => {
    const [info, setInfo] = useState<BuildInfo | null>(null);
    const [failed, setFailed] = useState(false);
    const [attempt, setAttempt] = useState(0);

    useEffect(() => {
        let active = true;
        setFailed(false);
        invoke<BuildInfo>("get_build_info").then(result => {
            if (active) setInfo(result);
        }).catch(() => {
            if (active) setFailed(true);
        });
        return () => { active = false; };
    }, [attempt]);

    const stateKey = info?.source_state === "verified_checkout" ? "build_source_verified"
        : info?.source_state === "modified_checkout" ? "build_source_modified" : "build_source_unknown";

    return (
        <section className="about-build" aria-labelledby="about-build-title">
            <div className="setting-item settings-section-title">
                <div className="item-label-group"><span className="item-label" id="about-build-title">{t("build_info")}</span></div>
            </div>
            {failed && <div className="about-build-error" role="alert">
                <p>{t("build_info_unavailable")}</p>
                <button type="button" className="setting-btn" onClick={() => setAttempt(value => value + 1)}>
                    {t("build_info_retry")}
                </button>
            </div>}
            {!failed && !info && <p className="update-note" role="status">{t("build_info_loading")}</p>}
            {info && <SettingsDisclosure className="about-build-details" title={t("build_details")}>
                <dl className="settings-section-grid">
                    {[
                        [t("app_version"), info.component_version],
                        [t("build_identifier"), info.build_id ?? "—"],
                        [t("build_source_revision"), info.source_revision ?? "—"],
                        [t("build_source_state"), t(stateKey)],
                        [t("build_target"), info.target],
                        [t("build_protocol"), `${info.protocol_version.major}.${info.protocol_version.minor}`],
                        [t("build_database_schema"), String(info.database_schema_version)],
                        ...info.linked_components.map(item => [item.component, item.version]),
                    ].map(([label, value], index) => <div className={`setting-item${index === 1 || index === 2 || index === 4 ? " settings-row--wide" : ""}`} key={label}>
                        <dt className="item-label-group">{label}</dt><dd className="settings-value">{value}</dd>
                    </div>)}
                </dl>
            </SettingsDisclosure>}
        </section>
    );
};

interface AboutSettingsGroupProps {
    readonly t: (key: string) => string;
    readonly collapsed: boolean;
    readonly collapsible: boolean;
    readonly onToggle: () => void;
}

const AboutSettingsGroup = ({ t, collapsed, collapsible, onToggle }: AboutSettingsGroupProps) => (
    <div className={`settings-group about-settings-group ${collapsed ? "collapsed" : ""}`}>
        <div className="group-header">
            <h3>{collapsible ? <button type="button" className="about-group-toggle" onClick={onToggle} aria-expanded={!collapsed}>
                {t("about_settings")}
                {collapsed ? <ChevronRight size={16} /> : <ChevronDown size={16} />}
            </button> : t("about_settings")}</h3>
        </div>
        {!collapsed && <div className="group-content">
            <BuildInfoSection t={t} />
            <UpdateSettingsSection t={t} />
        </div>}
    </div>
);

export default AboutSettingsGroup;
