import { SettingsToggle } from "@deziro/ui-react/components/SettingsToggle";
import { useEffect, useState } from "react";
import type { ComponentType, ReactNode } from "react";
import { invoke } from "../../../../shared/lib/desktopInvoke";
import { ChevronDown, ChevronRight } from "lucide-react";
import WaylandCapabilitiesSection from "./WaylandCapabilitiesSection";
import NativeFeatureBoundary from "../NativeFeatureBoundary";
import KeyboardPreviewSetting from "./KeyboardPreviewSetting";

interface LabelWithHintProps {
    label: string;
    hint?: string | ReactNode;
    hintKey: string;
}

interface GeneralSettingsGroupProps {
    t: (key: string) => string;
    collapsed: boolean;
    onToggle: () => void;
    LabelWithHint: ComponentType<LabelWithHintProps>;
    autoStart: boolean;
    setAutoStart: (val: boolean) => void;
    silentStart: boolean;
    setSilentStart: (val: boolean) => void;
    hideTrayIcon: boolean;
    setHideTrayIcon: (val: boolean) => void;
    edgeDocking: boolean;
    setEdgeDocking: (val: boolean) => void;
    followMouse: boolean;
    setFollowMouse: (val: boolean) => void;
    disableWebviewGpu: boolean;
    setDisableWebviewGpu: (val: boolean) => void;
    idleDestroyEnabled: boolean;
    setIdleDestroyEnabled: (val: boolean) => void;
    idleDestroySeconds: number;
    setIdleDestroySeconds: (val: number) => void;
    soundEnabled: boolean;
    setSoundEnabled: (val: boolean) => void;
    soundVolume: number;
    setSoundVolume: (val: number) => void;
    pasteSoundEnabled: boolean;
    setPasteSoundEnabled: (val: boolean) => void;
    showSearchBox: boolean;
    setShowSearchBox: (val: boolean) => void;
    scrollTopButtonEnabled: boolean;
    setScrollTopButtonEnabled: (val: boolean) => void;
    emojiPanelEnabled: boolean;
    setEmojiPanelEnabled: (val: boolean) => void;
    tagManagerEnabled: boolean;
    setTagManagerEnabled: (val: boolean) => void;
    arrowKeySelection: boolean;
    setArrowKeySelection: (val: boolean) => void;
    saveAppSetting: (key: string, val: string) => void;
}

const GeneralSettingsGroup = ({
    t,
    collapsed,
    onToggle,
    LabelWithHint,
    autoStart,
    setAutoStart,
    silentStart,
    setSilentStart,
    hideTrayIcon,
    setHideTrayIcon,
    edgeDocking,
    setEdgeDocking,
    followMouse,
    setFollowMouse,
    disableWebviewGpu,
    setDisableWebviewGpu,
    idleDestroyEnabled,
    setIdleDestroyEnabled,
    idleDestroySeconds,
    setIdleDestroySeconds,
    soundEnabled,
    setSoundEnabled,
    soundVolume,
    setSoundVolume,
    pasteSoundEnabled,
    setPasteSoundEnabled,
    showSearchBox,
    setShowSearchBox,
    scrollTopButtonEnabled,
    setScrollTopButtonEnabled,
    emojiPanelEnabled,
    setEmojiPanelEnabled,
    tagManagerEnabled,
    setTagManagerEnabled,
    arrowKeySelection,
    setArrowKeySelection,
    saveAppSetting
}: GeneralSettingsGroupProps) => {
    const [loginSaving, setLoginSaving] = useState(false);
    const [loginError, setLoginError] = useState("");
    const [traySaving, setTraySaving] = useState(false);
    const [trayError, setTrayError] = useState("");
    const [dockSaving, setDockSaving] = useState(false);
    const [idleSaving, setIdleSaving] = useState(false);
    const [idleError, setIdleError] = useState("");
    const [dockError, setDockError] = useState("");
    const [gpuSaving, setGpuSaving] = useState(false);
    const [gpuError, setGpuError] = useState("");
    const [soundSaving, setSoundSaving] = useState(false);
    const [soundError, setSoundError] = useState("");
    const commitSound = async (enabled: boolean, pasteOnly: boolean) => {
        setSoundSaving(true);
        setSoundError("");
        try {
            if (pasteOnly) {
                await invoke("save_setting", { key: "app.sound_paste_enabled", value: String(enabled) });
                setPasteSoundEnabled(enabled);
            } else {
                await invoke("set_sound_enabled", { enabled });
                setSoundEnabled(enabled);
            }
        } catch (error) {
            setSoundError(String(error));
        } finally {
            setSoundSaving(false);
        }
    };
    const [followSaving, setFollowSaving] = useState(false);
    const [followError, setFollowError] = useState("");
    const [silentSaving, setSilentSaving] = useState(false);
    const [silentError, setSilentError] = useState("");
    const [idleDestroyDraft, setIdleDestroyDraft] = useState(idleDestroySeconds.toString());
    useEffect(() => {
        setIdleDestroyDraft(idleDestroySeconds.toString());
    }, [idleDestroySeconds]);
    const commitIdleDestroy = async (rawValue?: string) => {
        const source = rawValue ?? idleDestroyDraft;
        if (!/^\d+$/.test(source)) {
            setIdleDestroyDraft(idleDestroySeconds.toString());
            return;
        }
        const parsed = parseInt(source, 10);
        const clamped = Math.max(5, Math.min(3600, parsed));
        setIdleDestroyDraft(clamped.toString());
        if (clamped !== idleDestroySeconds) {
            setIdleSaving(true);
            setIdleError("");
            try {
                await invoke("set_idle_destroy_seconds", { seconds: clamped });
                setIdleDestroySeconds(clamped);
            } catch (error) {
                setIdleDestroyDraft(String(idleDestroySeconds));
                setIdleError(String(error));
            } finally {
                setIdleSaving(false);
            }
        }
    };

    return (
    <div className={`settings-group ${collapsed ? 'collapsed' : ''}`}>
        <div className="group-header" onClick={onToggle}>
            <h3 style={{ margin: 0 }}>{t('general_settings')}</h3>
            {collapsed ? <ChevronRight size={16} /> : <ChevronDown size={16} />}
        </div>
        {!collapsed && (
            <div className="group-content">
                <WaylandCapabilitiesSection t={t} />
                <div className="setting-item">
                    <div className="item-label-group">
                        <span className="item-label">{t('autostart')}</span>
                    </div>
                    <SettingsToggle aria-label={t("autostart")}
                            checked={autoStart}
                            disabled={loginSaving}
                            onChange={async (e) => {
                                const enabled = e.target.checked;
                                setLoginSaving(true);
                                setLoginError("");
                                try {
                                    await invoke("toggle_autostart", { enabled });
                                    setAutoStart(await invoke<boolean>("is_autostart_enabled"));
                                } catch (error) {
                                    setLoginError(String(error));
                                } finally {
                                    setLoginSaving(false);
                                }
                            }} />
                    {loginError && <span role="alert">{t("common_error")} · {loginError}</span>}
                </div>

                <div className="setting-item">
                    <div className="item-label-group">
                        <span className="item-label">{t('hide_tray_icon')}</span>
                    </div>
                    <SettingsToggle checked={hideTrayIcon}
                            aria-label={t('hide_tray_icon')}
                            disabled={traySaving}
                            onChange={async (e) => {
                                const val = e.target.checked;
                                setTraySaving(true);
                                setTrayError("");
                                try {
                                    await invoke("set_tray_visible", { visible: !val });
                                    setHideTrayIcon(val);
                                } catch (error) {
                                    setTrayError(String(error));
                                } finally {
                                    setTraySaving(false);
                                }
                            }} />
                    {trayError && <span role="alert">{trayError}</span>}
                </div>

                <NativeFeatureBoundary t={t}>
                <div className="setting-item">
                    <LabelWithHint
                        label={t('edge_docking')}
                        hintKey="edge_docking"
                    />
                    <SettingsToggle checked={edgeDocking}
                            aria-label={t('edge_docking')}
                            disabled={dockSaving}
                            onChange={async (e) => {
                                const enabled = e.target.checked;
                                setDockSaving(true);
                                setDockError("");
                                try {
                                    await invoke("set_edge_docking", { enabled });
                                    setEdgeDocking(enabled);
                                } catch (error) {
                                    setDockError(String(error));
                                } finally {
                                    setDockSaving(false);
                                }
                            }} />
                    {dockError && <span role="alert">{dockError}</span>}
                </div>

                <div className="setting-item">
                    <div className="item-label-group">
                        <span className="item-label">{t('follow_mouse')}</span>
                    </div>
                    <SettingsToggle checked={followMouse}
                            aria-label={t('follow_mouse')}
                            disabled={followSaving}
                            onChange={async (e) => {
                                const enabled = e.target.checked;
                                setFollowSaving(true);
                                setFollowError("");
                                try {
                                    await invoke("set_follow_mouse", { enabled });
                                    setFollowMouse(enabled);
                                } catch (error) {
                                    setFollowError(String(error));
                                } finally {
                                    setFollowSaving(false);
                                }
                            }} />
                    {followError && <span role="alert">{t("common_error")} · {followError}</span>}
                </div>

                </NativeFeatureBoundary>
                <div className="setting-item">
                    <LabelWithHint
                        label={t('disable_webview_gpu')}
                        hintKey="disable_webview_gpu"
                    />
                    <SettingsToggle checked={disableWebviewGpu}
                            aria-label={t('disable_webview_gpu')}
                            disabled={gpuSaving}
                            onChange={async (e) => {
                                const enabled = e.target.checked;
                                setGpuSaving(true);
                                setGpuError("");
                                try {
                                    await invoke("save_setting", { key: "app.disable_webview_gpu", value: String(enabled) });
                                    setDisableWebviewGpu(enabled);
                                } catch (error) {
                                    setGpuError(String(error));
                                } finally {
                                    setGpuSaving(false);
                                }
                            }} />
                    {gpuError && <span role="alert">{t("common_error")} · {gpuError}</span>}
                </div>

                <div className="setting-item">
                    <LabelWithHint
                        label={t('idle_destroy_enabled')}
                        hintKey="idle_destroy_enabled"
                    />
                    <SettingsToggle aria-label={t('idle_destroy_enabled')}
                            checked={idleDestroyEnabled}
                            disabled={idleSaving}
                            onChange={async (e) => {
                                const enabled = e.target.checked;
                                setIdleSaving(true);
                                setIdleError("");
                                try {
                                    await invoke("set_idle_destroy_enabled", { enabled });
                                    setIdleDestroyEnabled(enabled);
                                } catch (error) {
                                    setIdleError(String(error));
                                } finally {
                                    setIdleSaving(false);
                                }
                            }} />
                </div>
                {idleError && <span role="alert">{t("common_error")} · {idleError}</span>}
                {idleDestroyEnabled && (
                    <div className="setting-item setting-item--detail">
                        <LabelWithHint
                            label={t('idle_destroy_seconds')}
                            hintKey="idle_destroy_seconds"
                        />
                        <input
                            className="numeric-input"
                            type="text"
                            inputMode="numeric"
                            pattern="[0-9]*"
                            aria-label={t('idle_destroy_seconds')}
                            disabled={idleSaving}
                            value={idleDestroyDraft}
                            onFocus={(e) => {
                                e.target.select();
                                invoke("focus_clipboard_window").catch(console.error);
                            }}
                            onChange={(e) => {
                                const next = e.target.value;
                                if (next === "" || /^\d+$/.test(next)) {
                                    setIdleDestroyDraft(next);
                                }
                            }}
                            onBlur={(e) => commitIdleDestroy(e.target.value)}
                            onKeyDown={(e) => {
                                if (e.key === 'Enter') {
                                    e.currentTarget.blur();
                                }
                            }}
                        />
                    </div>
                )}

                <div className="setting-item">
                    <div className="item-label-group">
                        <span className="item-label">{t('sound_effects') || "Sound Effects"}</span>
                    </div>
                    <SettingsToggle checked={soundEnabled}
                            aria-label={t('sound_effects')}
                            disabled={soundSaving}
                            onChange={(e) => void commitSound(e.target.checked, false)} />
                    {soundError && <span role="alert">{t("common_error")} · {soundError}</span>}
                </div>
                {soundEnabled && (
                    <div className="setting-item setting-item--detail">
                        <div className="item-label-group" style={{ flex: 1 }}>
                            <span className="item-label">{t('sound_volume') || "Sound Volume"}</span>
                            <span className="item-desc">{soundVolume}%</span>
                        </div>
                        <input
                            type="range"
                            min="0"
                            max="100"
                            step="1"
                            aria-label={t("sound_volume")}
                            value={soundVolume}
                            onChange={(e) => {
                                const volume = Number(e.target.value);
                                setSoundVolume(volume);
                            }}
                            style={{ width: '140px' }}
                        />
                    </div>
                )}
                {soundEnabled && (
                    <div className="setting-item setting-item--detail">
                        <div className="item-label-group">
                            <span className="item-label">{t('paste_sound') || "Paste Sound"}</span>
                        </div>
                        <SettingsToggle checked={pasteSoundEnabled}
                                aria-label={t('paste_sound')}
                                disabled={soundSaving}
                                onChange={(e) => void commitSound(e.target.checked, true)} />
                    </div>
                )}


                <div className="setting-item">
                    <LabelWithHint
                        label={t('silent_start')}
                        hintKey="silent_start"
                    />
                    <SettingsToggle checked={silentStart}
                            aria-label={t('silent_start')}
                            disabled={silentSaving}
                            onChange={async (e) => {
                                const enabled = e.target.checked;
                                setSilentSaving(true);
                                setSilentError("");
                                try {
                                    await invoke("set_silent_start", { enabled });
                                    setSilentStart(enabled);
                                } catch (error) {
                                    setSilentError(String(error));
                                } finally {
                                    setSilentSaving(false);
                                }
                            }} />
                    {silentError && <span role="alert">{silentError}</span>}
                </div>
                <div className="setting-item">
                    <LabelWithHint
                        label={t('show_search_box')}
                        hintKey="show_search_box"
                    />
                    <SettingsToggle checked={showSearchBox}
                            onChange={(e) => {
                                const enabled = e.target.checked;
                                setShowSearchBox(enabled);
                                saveAppSetting('show_search_box', String(enabled));
                            }} />
                </div>
                <div className="setting-item">
                    <LabelWithHint
                        label={t('scroll_top_button')}
                        hintKey="scroll_top_button"
                    />
                    <SettingsToggle checked={scrollTopButtonEnabled}
                            onChange={(e) => {
                                const enabled = e.target.checked;
                                setScrollTopButtonEnabled(enabled);
                                saveAppSetting('show_scroll_top_button', String(enabled));
                            }} />
                </div>
                <div className="setting-item">
                    <LabelWithHint
                        label={t('emoji_panel_enabled') || '表情包开关'}
                        hintKey="emoji_panel_enabled"
                    />
                    <SettingsToggle checked={emojiPanelEnabled}
                            onChange={(e) => {
                                const enabled = e.target.checked;
                                setEmojiPanelEnabled(enabled);
                                saveAppSetting('emoji_panel_enabled', String(enabled));
                            }} />
                </div>
                <div className="setting-item">
                    <LabelWithHint
                        label={t('tag_manager_enabled') || '标签管理页开关'}
                        hintKey="tag_manager_enabled"
                    />
                    <SettingsToggle checked={tagManagerEnabled}
                            aria-label={t('tag_manager_enabled')}
                            onChange={(e) => {
                                const enabled = e.target.checked;
                                setTagManagerEnabled(enabled);
                                saveAppSetting('tag_manager_enabled', String(enabled));
                            }} />
                </div>
                <div className="setting-item">
                    <LabelWithHint
                        label={t('arrow_key_selection')}
                        hintKey="arrow_key_selection"
                    />
                    <SettingsToggle checked={arrowKeySelection}
                            onChange={(e) => {
                                const enabled = e.target.checked;
                                setArrowKeySelection(enabled);
                                saveAppSetting('arrow_key_selection', String(enabled));
                            }} />
                </div>

                <KeyboardPreviewSetting t={t} arrowKeySelection={arrowKeySelection} />

                {/* Restart as Admin button */}
                <div className="setting-item">
                    <LabelWithHint
                        label={t('restart_as_admin') || "Restart as Admin"}
                        hintKey="restart_as_admin"
                    />
                    <button
                        className="btn-icon settings-action"
                        onClick={() => {
                            invoke("restart_as_admin").catch((err) => {
                                console.error("Failed to restart as admin:", err);
                            });
                        }}
                    >
                        {t('restart') || "Restart"}
                    </button>
                </div>
            </div>
        )}
    </div>
    );
};

export default GeneralSettingsGroup;
