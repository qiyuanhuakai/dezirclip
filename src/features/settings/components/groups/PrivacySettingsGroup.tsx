import { useEffect, useRef, useState, type ComponentType } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { invoke } from "../../../../shared/lib/desktopInvoke";
import { refreshPrivacyState } from "../../../../shared/lib/privacySession";
import { PrivacySessionSection } from "./PrivacySessionSection";
import PrivacyRulesManager from "./PrivacyRulesManager";
import type { LabelWithHintProps } from "../SettingsPanel.types";

interface PrivacySettingsGroupProps {
    t: (key: string) => string;
    collapsed: boolean;
    onToggle: () => void;
    LabelWithHint: ComponentType<LabelWithHintProps>;
    appSettings: Record<string, string>;
    saveAppSetting: (key: string, value: string) => void;
    privacyProtection: boolean;
    setPrivacyProtection: (val: boolean) => void;
    privacyProtectionKinds: string[];
    setPrivacyProtectionKinds: (val: string[]) => void;
    privacyProtectionCustomRules: string;
    setPrivacyProtectionCustomRules: (val: string) => void;
    privacyKindsOpen: boolean;
    setPrivacyKindsOpen: (val: boolean) => void;
    privacyRulesOpen: boolean;
    setPrivacyRulesOpen: (val: boolean) => void;
}

export default function PrivacySettingsGroup(props: PrivacySettingsGroupProps) {
    const [rulesDraft, setRulesDraft] = useState<string | null>(null);
    const [rulesSaveFailed, setRulesSaveFailed] = useState(false);
    const pendingRules = useRef<string | null>(null);
    const editingRules = useRef(false);
    const savingRules = useRef(false);
    const rulesTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
    const flushRules = async () => {
        if (savingRules.current) return;
        savingRules.current = true;
        try {
            while (pendingRules.current !== null && rulesTimer.current === null) {
                const rules: string = pendingRules.current;
                if (import.meta.env.MODE === "mygo") await refreshPrivacyState();
                await invoke("set_privacy_protection_custom_rules", { rules });
                if (pendingRules.current === rules) {
                    pendingRules.current = null;
                    props.setPrivacyProtectionCustomRules(rules);
                    if (!editingRules.current) setRulesDraft(null);
                }
            }
        } catch {
            setRulesSaveFailed(true);
        } finally {
            savingRules.current = false;
        }
    };

    const saveRules = (value: string) => {
        pendingRules.current = value;
        setRulesDraft(value);
        setRulesSaveFailed(false);
        if (rulesTimer.current !== null) clearTimeout(rulesTimer.current);
        rulesTimer.current = setTimeout(() => {
            rulesTimer.current = null;
            void flushRules();
        }, 250);
    };

    const excludedSourceApps = props.appSettings['app.excluded_source_apps'] || '';
    const [excludedSourceAppsDraft, setExcludedSourceAppsDraft] = useState(excludedSourceApps);

    useEffect(() => {
        setExcludedSourceAppsDraft(excludedSourceApps);
    }, [excludedSourceApps]);
    return <div className={`settings-group ${props.collapsed ? "collapsed" : ""}`}>
        <div className="group-header" onClick={props.onToggle}>
            <h3>{props.t("privacy_settings")}</h3>
            {props.collapsed ? <ChevronRight size={16} /> : <ChevronDown size={16} />}
        </div>
        {!props.collapsed && <div className="group-content">
                    <PrivacySessionSection t={props.t} appSettings={props.appSettings} />
                    <PrivacyRulesManager t={props.t} />
                    <div className="setting-item">
                        <props.LabelWithHint
                            label={props.t('privacy_protection')}
                            hintKey="privacy_protection"
                        />
                        <label className="switch">
                            <input
                                className="cb"
                                type="checkbox"
                                checked={props.privacyProtection}
                                onChange={(e) => {
                                    const val = e.target.checked;
                                    props.setPrivacyProtection(val);
                                    invoke('set_privacy_protection', { enabled: val }).catch(console.error);
                                }}
                            />
                            <div className="toggle"><div className="left" /><div className="right" /></div>
                        </label>
                    </div>

                    <div className="setting-item column source-exclusion-setting">
                        <props.LabelWithHint
                            label={props.t('excluded_source_apps')}
                            hintKey="excluded_source_apps"
                            hint={<>{props.t("excluded_source_apps_hint")}<br />{props.t("privacy_rules_legacy_exclusions_hint")}</>}
                        />
                        <textarea
                            className="search-input source-exclusion-input"
                            aria-label={props.t('excluded_source_apps')}
                            placeholder={props.t('excluded_source_apps_placeholder')}
                            value={excludedSourceAppsDraft}
                            onFocus={() => invoke('focus_clipboard_window').catch(console.error)}
                            onChange={(event) => setExcludedSourceAppsDraft(event.target.value)}
                            onBlur={() => props.saveAppSetting('excluded_source_apps', excludedSourceAppsDraft)}
                            spellCheck={false}
                        />
                    </div>

                    <div className="setting-item column">
                        <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                            <button
                                type="button"
                                className="btn-icon"
                                onClick={() => props.setPrivacyKindsOpen(!props.privacyKindsOpen)}
                                aria-label={props.t('privacy_protection_kinds')}
                                aria-expanded={props.privacyKindsOpen}
                                style={{ width: '24px', height: '24px' }}
                            >
                                {props.privacyKindsOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                            </button>
                            <span className="item-label" style={{ fontWeight: 400 }}>{props.t('privacy_protection_kinds')}</span>
                        </div>
                        {props.privacyKindsOpen && (
                            <div style={{ display: 'flex', flexWrap: 'wrap', gap: '8px', marginLeft: '30px' }}>
                                {[
                                    { id: 'phone', label: props.t('privacy_kind_phone') },
                                    { id: 'idcard', label: props.t('privacy_kind_idcard') },
                                    { id: 'email', label: props.t('privacy_kind_email') },
                                    { id: 'secret', label: props.t('privacy_kind_secret') },
                                    { id: 'password', label: props.t('privacy_kind_password') || "Strong Password" },
                                ].map(opt => {
                                    const checked = props.privacyProtectionKinds.includes(opt.id);
                                    return (
                                        <label key={opt.id} className="privacy-kind-option">
                                            <span className="switch">
                                            <input
                                                className="cb"
                                                type="checkbox"
                                                checked={checked}
                                                onChange={(e) => {
                                                    const next = e.target.checked
                                                        ? [...props.privacyProtectionKinds, opt.id]
                                                        : props.privacyProtectionKinds.filter(t => t !== opt.id);
                                                    props.setPrivacyProtectionKinds(next);
                                                    invoke('set_privacy_protection_kinds', { kinds: next }).catch(console.error);
                                                }}
                                            />
                                            <span className="toggle"><span className="left" /><span className="right" /></span>
                                            </span>
                                            <span>{opt.label}</span>
                                        </label>
                                    );
                                })}
                            </div>
                        )}
                    </div>

                    <div className="setting-item column">
                        <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                            <button
                                type="button"
                                className="btn-icon"
                                onClick={() => props.setPrivacyRulesOpen(!props.privacyRulesOpen)}
                                aria-label={props.t('privacy_protection_custom_rules')}
                                aria-expanded={props.privacyRulesOpen}
                                style={{ width: '24px', height: '24px' }}
                            >
                                {props.privacyRulesOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                            </button>
                            <props.LabelWithHint
                                label={props.t('privacy_protection_custom_rules')}
                                hintKey="privacy_protection_custom_rules"
                                labelStyle={{ fontWeight: 400 }}
                            />
                        </div>
                        {props.privacyRulesOpen && (
                            <textarea
                                className="search-input"
                                style={{ width: 'calc(100% - 30px)', maxWidth: '100%', minHeight: '80px', padding: '8px', borderRadius: '0', marginLeft: '30px', boxSizing: 'border-box' }}
                                aria-label={props.t("privacy_protection_custom_rules")}
                                placeholder={props.t('privacy_protection_custom_rules_placeholder')}
                                value={rulesDraft ?? props.privacyProtectionCustomRules}
                                onFocus={() => {
                                    editingRules.current = true;
                                    if (import.meta.env.MODE !== "mygo") void invoke("focus_clipboard_window").catch(console.error);
                                }}
                                onBlur={() => {
                                    editingRules.current = false;
                                    if (rulesTimer.current !== null) {
                                        clearTimeout(rulesTimer.current);
                                        rulesTimer.current = null;
                                        void flushRules();
                                    }
                                    if (pendingRules.current === null) setRulesDraft(null);
                                }}
                                onChange={e => { void saveRules(e.target.value); }}
                            />
                        )}
                        {rulesSaveFailed && rulesDraft !== null && (
                            <p role="alert">{props.t("privacy_rules_save_failed")}{" "}
                                <button type="button" className="btn-icon settings-action" onClick={() => { void saveRules(rulesDraft); }}>
                                    {props.t("common_retry")}
                                </button>
                            </p>
                        )}
                    </div>

        </div>}
    </div>;
}
