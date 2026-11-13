import NativeFeatureBoundary from "../NativeFeatureBoundary";
import { useEffect, useRef, useState } from "react";
import type { ComponentType, CSSProperties, ReactNode } from "react";
import { invoke } from "../../../../shared/lib/desktopInvoke";
import { refreshPrivacyState } from "../../../../shared/lib/privacySession";
import { ask, message } from "@tauri-apps/plugin-dialog";
import { ChevronDown, ChevronRight } from "lucide-react";
import ThemedSelect from "../ThemedSelect";
import { resolveRichTextSnapshotCacheLimit } from "../../../../shared/lib/richTextSnapshotCache";
import { setRichTextSnapshotCacheLimit as applySnapshotCacheLimit } from "../../../../shared/lib/richTextSnapshot";
import HotkeyListEditor from "../HotkeyListEditor";
import { PrivacySessionSection } from "./PrivacySessionSection";
import PrivacyRulesManager from "./PrivacyRulesManager";

interface LabelWithHintProps {
    label: string;
    hint?: string | ReactNode;
    hintKey: string;
    labelStyle?: CSSProperties;
}

interface ClipboardSettingsGroupProps {
    t: (key: string) => string;
    collapsed: boolean;
    onToggle: () => void;
    LabelWithHint: ComponentType<LabelWithHintProps>;
    persistent: boolean;
    setPersistent: (val: boolean) => void;
    persistentLimitEnabled: boolean;
    setPersistentLimitEnabled: (val: boolean) => void;
    persistentLimit: number;
    setPersistentLimit: (val: number) => void;
    saveAppSetting: (key: string, val: string) => void;
    deduplicate: boolean;
    setDeduplicate: (val: boolean) => void;
    captureFiles: boolean;
    setCaptureFiles: (val: boolean) => void;
    captureRichText: boolean;
    setCaptureRichText: (val: boolean) => void;
    richTextSnapshotPreview: boolean;
    setRichTextSnapshotPreview: (val: boolean) => void;
    richTextSnapshotCacheLimit: number;
    setRichTextSnapshotCacheLimit: (val: number) => void;
    richPasteHotkey: string;
    isRecordingRich: boolean;
    setIsRecordingRich: (val: boolean) => void;
    updateRichPasteHotkey: (key: string) => void;
    searchHotkey: string;
    isRecordingSearch: boolean;
    setIsRecordingSearch: (val: boolean) => void;
    updateSearchHotkey: (key: string) => void;
    deleteAfterPaste: boolean;
    setDeleteAfterPaste: (val: boolean) => void;
    moveToTopAfterPaste: boolean;
    setMoveToTopAfterPaste: (val: boolean) => void;
    pasteMethod: string;
    setPasteMethod: (val: string) => void;
    sequentialMode: boolean;
    pasteQueueCount: number;
    clearPasteQueue: () => void;
    setSequentialModeState: (val: boolean) => void;
    sequentialHotkey: string;
    isRecordingSequential: boolean;
    setIsRecordingSequential: (val: boolean) => void;
    updateSequentialHotkey: (key: string) => void;
    checkHotkeyConflict: (newHotkey: string, mode: 'main' | 'sequential' | 'rich' | 'search') => boolean;
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
    registryWinVEnabled: boolean;
    setRegistryWinVEnabled: (val: boolean) => void;
    isRecording: boolean;
    setIsRecording: (val: boolean) => void;
    mainHotkeys: string[];
    updateHotkey: (key: string) => void;
    addMainHotkey: (key: string, options?: { skipAvailabilityCheck?: boolean }) => Promise<boolean>;
    removeMainHotkey: (key: string) => Promise<boolean>;
    hotkey: string;
    appSettings: Record<string, string>;
    theme: string;
    colorMode: string;
}

const ClipboardSettingsGroup = (props: ClipboardSettingsGroupProps) => {
    const sequentialHotkeyParts = props.sequentialHotkey ? props.sequentialHotkey.split('+') : [];
    const searchHotkeyParts = props.searchHotkey ? props.searchHotkey.split('+') : [];
    const pasteMethodOptions = [
        { value: "shift_insert", label: props.t("paste_method_shift_insert") },
        { value: "ctrl_v", label: props.t("paste_method_ctrl_v") },
        { value: "game_mode", label: props.t("paste_method_game_mode") }
    ];

    const pasteMethodBusy = useRef(false);
    const [pasteMethodError, setPasteMethodError] = useState(false);
    const applyPasteMethod = async (val: string) => {
        if (pasteMethodBusy.current) return;
        pasteMethodBusy.current = true;
        setPasteMethodError(false);
        let staged = false;
        try {
            if (val === 'game_mode') {
                const isAdmin = await invoke<boolean>("check_is_admin");
                if (!isAdmin) {
                    const confirmed = await ask(
                        props.t('game_mode_admin_required') || "Game Mode requires Administrator privileges to work correctly with games (especially for IME/Input handling). Restart as Admin now?",
                        {
                            title: props.t('admin_required') || "Administrator Required",
                            kind: 'warning'
                        }
                    );

                    if (confirmed) {
                        await invoke("save_setting", { key: 'app.paste_method', value: 'game_mode' });
                        staged = true;
                        await invoke("restart_as_admin");
                        return;
                    } else {
                        return;
                    }
                }
            }
            await invoke("save_setting", { key: 'app.paste_method', value: val });
            props.setPasteMethod(val);
        } catch (err) {
            if (staged) {
                await invoke("save_setting", { key: 'app.paste_method', value: props.pasteMethod }).catch(console.error);
            }
            console.error("Failed to change paste method:", err);
            setPasteMethodError(true);
        } finally {
            pasteMethodBusy.current = false;
        }
    };
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

    const [registryWinVChanging, setRegistryWinVChanging] = useState(false);
    const [persistentLimitDraft, setPersistentLimitDraft] = useState(
        props.persistentLimit.toString()
    );
    const [snapshotCacheLimitDraft, setSnapshotCacheLimitDraft] = useState(
        props.richTextSnapshotCacheLimit.toString()
    );
    const excludedSourceApps = props.appSettings['app.excluded_source_apps'] || '';
    const [excludedSourceAppsDraft, setExcludedSourceAppsDraft] = useState(excludedSourceApps);

    useEffect(() => {
        setExcludedSourceAppsDraft(excludedSourceApps);
    }, [excludedSourceApps]);
    const isWinVHotkey = (value: string) => {
        const parts = value
            .split('+')
            .map((item) => item.trim().toUpperCase())
            .filter((item) => !!item);
        if (parts.length !== 2) return false;
        const hasWin = parts.includes('WIN') || parts.includes('SUPER') || parts.includes('META') || parts.includes('COMMAND');
        const hasV = parts.includes('V');
        return hasWin && hasV;
    };

    useEffect(() => {
        setPersistentLimitDraft(props.persistentLimit.toString());
    }, [props.persistentLimit, props.persistentLimitEnabled]);

    useEffect(() => {
        setSnapshotCacheLimitDraft(props.richTextSnapshotCacheLimit.toString());
    }, [props.richTextSnapshotCacheLimit]);

    // Blank or non-numeric input reverts instead of committing, so a
    // half-typed number never lands as 0 -- which would read as "caching
    // disabled" and quietly cost the memory the user meant to keep.
    const commitSnapshotCacheLimit = (rawValue?: string) => {
        const source = rawValue ?? snapshotCacheLimitDraft;
        if (!/^\d+$/.test(source)) {
            setSnapshotCacheLimitDraft(props.richTextSnapshotCacheLimit.toString());
            return;
        }
        const clamped = resolveRichTextSnapshotCacheLimit(source);
        setSnapshotCacheLimitDraft(clamped.toString());
        if (clamped !== props.richTextSnapshotCacheLimit) {
            props.setRichTextSnapshotCacheLimit(clamped);
            props.saveAppSetting('rich_text_snapshot_cache_limit', String(clamped));
            // The module owns the Map as well as the number, and only this
            // call reaches it -- React state and the settings row do not. Without
            // it the setting would not take effect until something else happened
            // to reload every setting, which for a user means "the number moved
            // and nothing did". Lowering it this way also hands the memory back
            // straight away, since the setter trims.
            applySnapshotCacheLimit(clamped);
        }
    };

    const [limitSaving, setLimitSaving] = useState(false);
    const [limitError, setLimitError] = useState("");
    const saveLimit = async (key: "persistent_limit" | "persistent_limit_enabled", value: string) => {
        setLimitSaving(true);
        setLimitError("");
        try {
            await invoke("save_setting", { key: "app." + key, value });
            if (key === "persistent_limit_enabled") props.setPersistentLimitEnabled(value === "true");
            else {
                props.setPersistentLimit(Number(value));
                setPersistentLimitDraft(value);
            }
        } catch (error) {
            setLimitError(String(error));
            setPersistentLimitDraft(String(props.persistentLimit));
        } finally {
            setLimitSaving(false);
        }
    };
    const commitPersistentLimit = () => {
        const parsed = Number.parseInt(persistentLimitDraft, 10);
        if (!Number.isFinite(parsed)) {
            setPersistentLimitDraft(String(props.persistentLimit));
            return;
        }
        const value = Math.max(50, Math.min(99999, parsed));
        if (value === props.persistentLimit) setPersistentLimitDraft(String(value));
        else void saveLimit("persistent_limit", String(value));
    };

    return (
        <div className={`settings-group ${props.collapsed ? 'collapsed' : ''}`}>
            <div className="group-header" onClick={props.onToggle}>
                <h3 style={{ margin: 0 }}>{props.t('clipboard_settings')}</h3>
                {props.collapsed ? <ChevronRight size={16} /> : <ChevronDown size={16} />}
            </div>
            {!props.collapsed && (
                <div className="group-content">
                    <div className="setting-item">
                        <props.LabelWithHint
                            label={props.t('persistent_storage')}
                            hintKey="persistent_storage"
                        />
                        <label className="switch">
                            <input
                                className="cb"
                                type="checkbox"
                                checked={props.persistent}
                                aria-label={props.t('persistent_storage')}
                                onChange={(e) => props.setPersistent(e.target.checked)}
                            />
                            <div className="toggle"><div className="left" /><div className="right" /></div>
                        </label>
                    </div>
                    {props.persistent && (
                        <>
                            <div className="setting-item">
                                <props.LabelWithHint
                                    label={props.t('persistent_limit_enabled')}
                                    hintKey="persistent_limit_enabled"
                                />
                                <label className="switch">
                                    <input
                                        className="cb"
                                        type="checkbox"
                                        checked={props.persistentLimitEnabled}
                                        aria-label={props.t('persistent_limit_enabled')}
                                        disabled={limitSaving}
                                        onChange={(e) => void saveLimit("persistent_limit_enabled", String(e.target.checked))}
                                    />
                                    <div className="toggle"><div className="left" /><div className="right" /></div>
                                </label>
                            </div>
                            {limitError && <span role="alert">{props.t("common_error")} · {limitError}</span>}
                            {props.persistentLimitEnabled && (
                                <div className="setting-item">
                                    <props.LabelWithHint
                                        label={props.t('persistent_limit')}
                                        hintKey="persistent_limit"
                                    />
                                    <input
                                        className="numeric-input"
                                        type="text"
                                        inputMode="numeric"
                                        pattern="[0-9]*"
                                        aria-label={props.t('persistent_limit')}
                                        disabled={limitSaving}
                                        value={persistentLimitDraft}
                                        onFocus={(e) => {
                                            e.target.select();
                                            invoke("focus_clipboard_window").catch(console.error);
                                        }}
                                        onChange={(e) => {
                                            const next = e.target.value;
                                            if (next === "") {
                                                setPersistentLimitDraft("");
                                                return;
                                            }
                                            if (!/^\d+$/.test(next)) return;
                                            setPersistentLimitDraft(next);
                                        }}
                                        onBlur={() => {
                                            commitPersistentLimit();
                                        }}
                                        onKeyDown={(e) => {
                                            if (e.key === 'Enter') {
                                                e.currentTarget.blur();
                                            }
                                        }}
                                    />
                                </div>
                            )}
                        </>
                    )}
                    <div className="setting-item">
                        <props.LabelWithHint
                            label={props.t('merge_duplicates')}
                            hintKey="merge_duplicates"
                        />
                        <label className="switch">
                            <input
                                className="cb"
                                type="checkbox"
                                checked={props.deduplicate}
                                onChange={(e) => props.setDeduplicate(e.target.checked)}
                            />
                            <div className="toggle"><div className="left" /><div className="right" /></div>
                        </label>
                    </div>
                    <div className="setting-item">
                        <div className="item-label-group">
                            <span className="item-label">{props.t('capture_files')}</span>
                        </div>
                        <label className="switch">
                            <input
                                className="cb"
                                type="checkbox"
                                checked={props.captureFiles}
                                onChange={(e) => props.setCaptureFiles(e.target.checked)}
                            />
                            <div className="toggle"><div className="left" /><div className="right" /></div>
                        </label>
                    </div>
                    <div className="setting-item">
                        <props.LabelWithHint
                            label={props.t('capture_rich_text') || '捕获富文本'}
                            hintKey="capture_rich_text"
                        />
                        <label className="switch">
                            <input
                                className="cb"
                                type="checkbox"
                                checked={props.captureRichText}
                                onChange={(e) => {
                                    const val = e.target.checked;
                                    props.setCaptureRichText(val);
                                }}
                            />
                            <div className="toggle"><div className="left" /><div className="right" /></div>
                        </label>
                    </div>
                    <div className="setting-item">
                        <props.LabelWithHint
                            label={props.t('rich_text_snapshot_preview') || '富文本快照预览'}
                            hintKey="rich_text_snapshot_preview"
                        />
                        <label className="switch">
                            <input
                                className="cb"
                                type="checkbox"
                                checked={props.richTextSnapshotPreview}
                                aria-label={props.t('rich_text_snapshot_preview')}
                                onChange={(e) => {
                                    const val = e.target.checked;
                                    props.setRichTextSnapshotPreview(val);
                                    props.saveAppSetting('rich_text_snapshot_preview', String(val));
                                }}
                            />
                            <div className="toggle"><div className="left" /><div className="right" /></div>
                        </label>
                    </div>

                    {props.richTextSnapshotPreview && (
                        <div className="setting-item setting-item--detail">
                            <props.LabelWithHint
                                label={props.t('rich_text_snapshot_cache_limit')}
                                hintKey="rich_text_snapshot_cache_limit"
                            />
                            <input
                                className="numeric-input"
                                type="text"
                                inputMode="numeric"
                                pattern="[0-9]*"
                                value={snapshotCacheLimitDraft}
                                aria-label={props.t('rich_text_snapshot_cache_limit')}
                                onFocus={(e) => e.target.select()}
                                onChange={(e) => {
                                    const next = e.target.value;
                                    if (next === "" || /^\d+$/.test(next)) {
                                        setSnapshotCacheLimitDraft(next);
                                    }
                                }}
                                onBlur={(e) => commitSnapshotCacheLimit(e.target.value)}
                                onKeyDown={(e) => {
                                    if (e.key === 'Enter') {
                                        commitSnapshotCacheLimit(e.currentTarget.value);
                                        e.currentTarget.blur();
                                    }
                                }}
                            />
                        </div>
                    )}


                    <NativeFeatureBoundary t={props.t}>
                    <div className="setting-item">
                        <div className="item-label-group">
                            <span className="item-label">{props.t('rich_paste_hotkey_label')}</span>

                        </div>
                        <div
                            className={`key-group ${props.isRecordingRich ? 'recording' : ''}`}
                            onClick={(e) => { props.setIsRecordingRich(true); invoke("focus_clipboard_window").catch(console.error); e.currentTarget.focus(); }}
                            tabIndex={0}
                            onKeyDown={(e) => {
                                if (!props.isRecordingRich) return;
                                e.preventDefault();
                                e.stopPropagation();

                                if (e.key === 'Escape') {
                                    props.setIsRecordingRich(false);
                                    return;
                                }

                                const modifiers = [];
                                if (e.ctrlKey) modifiers.push('Ctrl');
                                if (e.shiftKey) modifiers.push('Shift');
                                if (e.altKey) modifiers.push('Alt');
                                if (e.metaKey) modifiers.push('Win');

                                const key = e.key.toUpperCase();
                                if (['CONTROL', 'SHIFT', 'ALT', 'META'].includes(key)) return;

                                const newHotkey = [...modifiers, key].join('+');
                                props.updateRichPasteHotkey(newHotkey);
                            }}
                            onMouseDown={(e) => {
                                if (!props.isRecordingRich) return;
                                e.preventDefault();
                                if (e.button === 1) {
                                    props.updateRichPasteHotkey('MouseMiddle');
                                }
                            }}
                        >
                            {props.isRecordingRich ? (
                                <div className="key-cap" style={{ width: '8em' }}>{props.t('waiting_for_input')}</div>
                            ) : (
                                (props.richPasteHotkey || '').split('+').filter(Boolean).map((k, i) => (
                                    <div key={i} className="key-cap">{k}</div>
                                ))
                            )}
                            {!props.isRecordingRich && !props.richPasteHotkey && (
                                <div className="key-cap" style={{ opacity: 0.5 }}>{props.t('not_set')}</div>
                            )}
                        </div>
                    </div>
                    <div className="setting-item">
                        <div className="item-label-group">
                            <span className="item-label">{props.t('search_hotkey_label')}</span>

                        </div>
                        <div
                            className={`key-group ${props.isRecordingSearch ? 'recording' : ''}`}
                            role="button"
                            aria-label={props.t('search_hotkey_label')}
                            aria-pressed={props.isRecordingSearch}
                            onClick={(e) => { props.setIsRecordingSearch(true); invoke("focus_clipboard_window").catch(console.error); e.currentTarget.focus(); }}
                            tabIndex={0}
                            onKeyDown={(e) => {
                                if (!props.isRecordingSearch) return;
                                e.preventDefault();
                                e.stopPropagation();

                                if (e.key === 'Escape') {
                                    props.setIsRecordingSearch(false);
                                    return;
                                }

                                const modifiers = [];
                                if (e.ctrlKey) modifiers.push('Ctrl');
                                if (e.shiftKey) modifiers.push('Shift');
                                if (e.altKey) modifiers.push('Alt');
                                if (e.metaKey) modifiers.push('Win');

                                const key = e.key.toUpperCase();
                                if (['CONTROL', 'SHIFT', 'ALT', 'META'].includes(key)) return;

                                const newHotkey = [...modifiers, key].join('+');
                                props.updateSearchHotkey(newHotkey);
                            }}
                            onMouseDown={(e) => {
                                if (!props.isRecordingSearch) return;
                                e.preventDefault();
                                if (e.button === 1) {
                                    props.updateSearchHotkey('MouseMiddle');
                                }
                            }}
                        >
                            {props.isRecordingSearch ? (
                                <div className="key-cap" style={{ width: '8em' }}>{props.t('waiting_for_input')}</div>
                            ) : (
                                searchHotkeyParts.length > 0 ? (
                                    searchHotkeyParts.map((k, i) => (
                                        <div key={i} className="key-cap">{k}</div>
                                    ))
                                ) : (
                                    <div className="key-cap" style={{ width: '8em', opacity: 0.5 }}>{props.t('not_set')}</div>
                                )
                            )}
                        </div>
                    </div>
                    <div className="setting-item">
                        <div className="item-label-group">
                            <span className="item-label">{props.t('delete_after_paste')}</span>
                        </div>
                        <label className="switch">
                            <input
                                className="cb"
                                type="checkbox"
                                checked={props.deleteAfterPaste}
                                onChange={(e) => {
                                    const val = e.target.checked;
                                    props.setDeleteAfterPaste(val);
                                    props.saveAppSetting('delete_after_paste', String(val));
                                }}
                            />
                            <div className="toggle"><div className="left" /><div className="right" /></div>
                        </label>
                    </div>
                    <div className="setting-item">
                        <props.LabelWithHint
                            label={props.t('move_to_top_after_paste')}
                            hintKey="move_to_top_after_paste"
                        />
                        <label className="switch">
                            <input
                                className="cb"
                                type="checkbox"
                                checked={props.moveToTopAfterPaste}
                                onChange={(e) => {
                                    const val = e.target.checked;
                                    props.setMoveToTopAfterPaste(val);
                                    props.saveAppSetting('move_to_top_after_paste', String(val));
                                }}
                            />
                            <div className="toggle"><div className="left" /><div className="right" /></div>
                        </label>
                    </div>
                    <div className="setting-item">
                        <props.LabelWithHint
                            label={props.t('paste_method')}
                            hintKey="paste_method"
                        />
                        <ThemedSelect
                            options={pasteMethodOptions}
                            value={props.pasteMethod}
                            width="124px"
                            onChange={applyPasteMethod}
                        />
                        {pasteMethodError && <span role="alert">{props.t('common_error')}</span>}
                    </div>
                    <div className="setting-item">
                        <props.LabelWithHint
                            label={props.t('sequential_paste_mode')}
                            hintKey="sequential_paste_mode"
                        />
                        <label className="switch">
                            <input
                                className="cb"
                                type="checkbox"
                                checked={props.sequentialMode}
                                onChange={(e) => {
                                    const val = e.target.checked;
                                    props.setSequentialModeState(val);
                                    invoke('set_sequential_mode', { enabled: val }).catch(console.error);
                                    // Turning the mode off used to leave the queue
                                    // intact, and the hotkey accepts a non-empty
                                    // queue even when the mode is off, so a batch
                                    // copied before the switch stayed pasteable.
                                    //
                                    // No `pasteQueueCount > 0` guard: that number
                                    // is refreshed on paste events and on window
                                    // focus, but copying into the queue emits
                                    // none of them, so it is routinely stale at
                                    // exactly this moment -- the batch this
                                    // branch exists to drop is the batch the
                                    // count cannot see. Clearing is idempotent,
                                    // and `clear_paste_queue` answers with
                                    // `queue-finished`, which is the refresh the
                                    // count was waiting for anyway.
                                    if (!val) {
                                        props.clearPasteQueue();
                                    }
                                    if (val) {
                                        if (props.checkHotkeyConflict(props.sequentialHotkey, 'sequential')) {
                                            props.updateSequentialHotkey("");
                                        }
                                    }
                                }}
                            />
                            <div className="toggle"><div className="left" /><div className="right" /></div>
                        </label>
                    </div>

                    {props.sequentialMode && props.pasteQueueCount > 0 && (
                        <div className="setting-item setting-item--detail">
                            <div className="item-label-group">
                                <span className="item-label">
                                    {props.t('sequential_paste_queue_count').replace('{count}', String(props.pasteQueueCount))}
                                </span>
                            </div>
                            <button
                                type="button"
                                className="setting-btn setting-btn--compact"
                                onClick={() => props.clearPasteQueue()}
                            >
                                {props.t('sequential_paste_clear_queue')}
                            </button>
                        </div>
                    )}

                    {props.sequentialMode && (
                        <div className="setting-item">
                            <div className="item-label-group">
                                <span className="item-label">{props.t('sequential_paste_hotkey_label')}</span>

                            </div>
                            <div
                                className={`key-group ${props.isRecordingSequential ? 'recording' : ''}`}
                                onClick={(e) => { props.setIsRecordingSequential(true); invoke("focus_clipboard_window").catch(console.error); e.currentTarget.focus(); }}
                                tabIndex={0}
                                onKeyDown={(e) => {
                                    if (!props.isRecordingSequential) return;
                                    e.preventDefault();
                                    e.stopPropagation();

                                    if (e.key === 'Escape') {
                                        props.setIsRecordingSequential(false);
                                        return;
                                    }

                                    const modifiers = [];
                                    if (e.ctrlKey) modifiers.push('Ctrl');
                                    if (e.shiftKey) modifiers.push('Shift');
                                    if (e.altKey) modifiers.push('Alt');
                                    if (e.metaKey) modifiers.push('Win');

                                    const key = e.key.toUpperCase();
                                    if (['CONTROL', 'SHIFT', 'ALT', 'META'].includes(key)) return;

                                    const newHotkey = [...modifiers, key].join('+');
                                    props.updateSequentialHotkey(newHotkey);
                                }}
                                onMouseDown={(e) => {
                                    if (!props.isRecordingSequential) return;
                                    e.preventDefault();
                                    if (e.button === 1) {
                                        props.updateSequentialHotkey('MouseMiddle');
                                    }
                                }}
                            >
                                {props.isRecordingSequential ? (
                                    <div className="key-cap" style={{ width: '8em' }}>{props.t('waiting_for_input')}</div>
                                ) : (
                                    sequentialHotkeyParts.length > 0 ? (
                                        sequentialHotkeyParts.map((k, i) => (
                                            <div key={i} className="key-cap">{k}</div>
                                        ))
                                    ) : (
                                        <div className="key-cap" style={{ width: '8em', opacity: 0.5 }}>{props.t('not_set')}</div>
                                    )
                                )}
                            </div>
                        </div>
                    )}

                    </NativeFeatureBoundary>
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

                    <div className="setting-item source-exclusion-setting">
                        <props.LabelWithHint
                            label={props.t('excluded_source_apps')}
                            hintKey="excluded_source_apps"
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
                        <p className="settings-description">{props.t("privacy_rules_legacy_exclusions_hint")}</p>
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
                                <button type="button" className="setting-btn" onClick={() => { void saveRules(rulesDraft); }}>
                                    {props.t("common_retry")}
                                </button>
                            </p>
                        )}
                    </div>

                    <NativeFeatureBoundary t={props.t}>
                    <HotkeyListEditor t={props.t} labelKey="global_hotkey" hotkeys={props.mainHotkeys}
                        recording={props.isRecording} setRecording={props.setIsRecording}
                        onAdd={props.addMainHotkey} onRemove={props.removeMainHotkey}
                        canRemove={item => !isWinVHotkey(item) || !props.registryWinVEnabled} />

                    <div className="setting-item">
                        <props.LabelWithHint
                            label={props.t('use_win_v_shortcut')}
                            hintKey="use_win_v_shortcut"
                        />
                        <label className="switch">
                            <input
                                className="cb"
                                type="checkbox"
                                checked={props.registryWinVEnabled}
                                disabled={registryWinVChanging}
                                onChange={async (e) => {
                                    if (registryWinVChanging) return;
                                    setRegistryWinVChanging(true);
                                    let rollbackToken: string | undefined;
                                    let explorerRestarted = false;
                                    const enabled = e.target.checked;
                                    const previousEnabled = props.registryWinVEnabled;
                                    const matchedWinV = props.mainHotkeys.find((item) => isWinVHotkey(item));
                                    const hasWinV = !!matchedWinV;
                                    props.setRegistryWinVEnabled(enabled);
                                    try {
                                        if (!enabled && matchedWinV) {
                                            const removed = await props.removeMainHotkey(matchedWinV);
                                            if (!removed) {
                                                props.setRegistryWinVEnabled(previousEnabled);
                                                setRegistryWinVChanging(false);
                                                return;
                                            }
                                        }

                                        await invoke("save_setting", { key: 'app.use_win_v_shortcut', value: String(enabled) });
                                        const [changed, token] = await invoke<[boolean, string]>("begin_registry_win_v_change", { enable: enabled });
                                        rollbackToken = token;

                                        if (changed) {
                                            const confirmed = await ask(
                                                props.t('restart_explorer_confirm'),
                                                { title: props.t('restart_explorer_title'), kind: 'warning' }
                                            );
                                            if (confirmed) {
                                                await invoke("restart_explorer");
                                                explorerRestarted = true;
                                            } else {
                                                if (enabled && !hasWinV) {
                                                    await props.removeMainHotkey("Win+V");
                                                }
                                                if (!enabled && matchedWinV) {
                                                    await props.addMainHotkey(matchedWinV, { skipAvailabilityCheck: true });
                                                }
                                                await invoke("save_setting", { key: 'app.use_win_v_shortcut', value: String(previousEnabled) });
                                                await invoke("finish_registry_win_v_change", { token: rollbackToken, rollback: true });
                                                rollbackToken = undefined;
                                                props.setRegistryWinVEnabled(previousEnabled);
                                            }
                                        }
                                        if (rollbackToken !== undefined) {
                                            // Explorer must release its system claim before MyGo can
                                            // acquire the same Windows shortcut. Keep the receipt until
                                            // registration succeeds so failures restore the original state.
                                            if (enabled && !hasWinV) {
                                                const added = await props.addMainHotkey("Win+V", { skipAvailabilityCheck: true });
                                                if (!added) throw new Error("Win+V registration was not completed");
                                            } else if (enabled) {
                                                await invoke("register_hotkey", { hotkey: props.mainHotkeys.join("\n") });
                                            }
                                            await invoke("finish_registry_win_v_change", { token: rollbackToken, rollback: false });
                                            rollbackToken = undefined;
                                            if (explorerRestarted) {
                                                setTimeout(async () => {
                                                    try {
                                                        await invoke("set_theme", {
                                                            theme: props.theme,
                                                            color_mode: props.colorMode,
                                                            show_app_border: props.appSettings["app.show_app_border"] !== "false"
                                                        });
                                                    } catch (e) {
                                                        console.error("Failed to restore theme:", e);
                                                    }
                                                }, 2500);
                                            }
                                        }
                                    } catch (err) {
                                        try {
                                            if (rollbackToken !== undefined) {
                                                await invoke("finish_registry_win_v_change", { token: rollbackToken, rollback: true });
                                                rollbackToken = undefined;
                                                if (explorerRestarted) await invoke("restart_explorer");
                                            }
                                        } catch (registryErr) {
                                            console.error("Rollback registry optimization failed:", registryErr);
                                        }
                                        try {
                                            if (enabled && !hasWinV) {
                                                await props.removeMainHotkey("Win+V");
                                            }
                                            if (!enabled && matchedWinV) {
                                                await props.addMainHotkey(matchedWinV, { skipAvailabilityCheck: true });
                                            }
                                        } catch (hotkeyErr) {
                                            console.error("Rollback Win+V hotkey failed:", hotkeyErr);
                                        }
                                        try {
                                            await invoke("save_setting", { key: 'app.use_win_v_shortcut', value: String(previousEnabled) });
                                        } catch (saveErr) {
                                            console.error("Rollback save_setting failed:", saveErr);
                                        }
                                        props.setRegistryWinVEnabled(previousEnabled);
                                        console.error(err);
                                        message(props.t('error') + `: ${err}`, { kind: 'error' });
                                    } finally {
                                        setRegistryWinVChanging(false);
                                    }
                                }}
                            />
                            <div className="toggle"><div className="left" /><div className="right" /></div>
                        </label>
                    </div>
                    </NativeFeatureBoundary>
                </div>
            )}
        </div>
    );
};

export default ClipboardSettingsGroup;
