import type { ComponentType } from "react";
import GeneralSettingsGroup from "./groups/GeneralSettingsGroup";
import ClipboardSettingsGroup from "./groups/ClipboardSettingsGroup";
import AppearanceSettingsGroup from "./groups/AppearanceSettingsGroup";
import DefaultAppsSettingsGroup from "./groups/DefaultAppsSettingsGroup";
import DataSettingsGroup from "./groups/DataSettingsGroup";
import ToolsSettingsGroup from "./groups/ToolsSettingsGroup";
import AboutSettingsGroup from "./groups/AboutSettingsGroup";
import type { LabelWithHintProps, SettingsPanelProps } from "./SettingsPanel.types";
import type { SettingsGroupId } from "../utils/settingsLayout";
import { isSettingsGroupCollapsed } from "../utils/settingsLayout";

interface SettingsGroupsProps {
    readonly settings: SettingsPanelProps;
    readonly LabelWithHint: ComponentType<LabelWithHintProps>;
    /** Wide layout: the rail picks the group, so only that one mounts. */
    readonly split: boolean;
    readonly activeGroup: SettingsGroupId;
    readonly privacyKindsOpen: boolean;
    readonly setPrivacyKindsOpen: (val: boolean) => void;
    readonly privacyRulesOpen: boolean;
    readonly setPrivacyRulesOpen: (val: boolean) => void;
    readonly screenshotEnabled: boolean;
    readonly setScreenshotEnabled: (val: boolean) => void;
    readonly screenshotHotkey: string;
    readonly quickPasteEnabled: boolean;
    readonly setQuickPasteEnabled: (val: boolean) => void;
    readonly ocrEnabled: boolean;
    readonly setOcrEnabled: (val: boolean) => void;
}

const SettingsGroups = ({
    settings,
    LabelWithHint,
    split,
    activeGroup,
    privacyKindsOpen,
    setPrivacyKindsOpen,
    privacyRulesOpen,
    setPrivacyRulesOpen,
    screenshotEnabled,
    setScreenshotEnabled,
    screenshotHotkey,
    quickPasteEnabled,
    setQuickPasteEnabled,
    ocrEnabled,
    setOcrEnabled,
}: SettingsGroupsProps) => {
    const { t, collapsedGroups, toggleGroup, saveAppSetting } = settings;

    // The narrow layout keeps the accordion: every group stays
    // mounted and `collapsedGroups` decides which bodies are visible.
    //
    // The split layout hands that choice to the rail -- only the active group is
    // shown -- and opens that one. A foldable pane there would be a second way to
    // change section that lands on a blank column with the rail still pointing at
    // it, so the header reads as a title instead: CSS drops its chevron and
    // pointer, and `onToggle` is a no-op. The stored flag is never written, which
    // is also what makes dragging the window narrow restore the accordion exactly
    // as it was left.
    //
    // The groups the rail is *not* pointing at keep their stored fold rather
    // than being forced open. Opening them meant mounting bodies nobody could see,
    // and those bodies start work on mount: `ToolsCaptureSection` probes for an
    // OCR engine, which on Linux means a `tesseract --version` subprocess, and
    // `AppearanceSettingsGroup` enumerates every installed font. Since the settings
    // page stays mounted while the clipboard list is showing, resizing across the
    // breakpoint used to run both behind a closed page, once per crossing.
    //
    // Hiding is done with the `hidden` attribute on a slot rather than by not
    // rendering the group. Unmounting was the wrong tool twice over: crossing the
    // 560px breakpoint unmounted whichever group the rail was not pointing at, and
    // `activeGroup` does not follow an accordion expansion, so opening the data
    // group in a narrow window and then widening it dropped that group anyway --
    // taking an open export/import dialog and its draft with it. `hidden` keeps
    // every group mounted and its local state alive; only the wrapper differs
    // between the two layouts. The same reasoning is why a hidden group honours
    // its stored fold: see `isSettingsGroupCollapsed`.
    const shown = (id: SettingsGroupId) => !split || activeGroup === id;
    const isCollapsed = (id: SettingsGroupId) =>
        isSettingsGroupCollapsed({
            split,
            shown: shown(id),
            collapsed: Boolean(collapsedGroups[id]),
        });
    const collapse = (id: SettingsGroupId) => {
        if (!split) toggleGroup(id);
    };

    return (
        <>
            <div className="settings-group-slot" hidden={!shown("general")}>
                <GeneralSettingsGroup
                    t={t}
                    collapsed={isCollapsed("general")}
                    onToggle={() => collapse("general")}
                    LabelWithHint={LabelWithHint}
                    autoStart={settings.autoStart}
                    setAutoStart={settings.setAutoStart}
                    silentStart={settings.silentStart}
                    setSilentStart={settings.setSilentStart}
                    hideTrayIcon={settings.hideTrayIcon}
                    setHideTrayIcon={settings.setHideTrayIcon}
                    edgeDocking={settings.edgeDocking}
                    setEdgeDocking={settings.setEdgeDocking}
                    followMouse={settings.followMouse}
                    setFollowMouse={settings.setFollowMouse}
                    disableWebviewGpu={settings.disableWebviewGpu}
                    setDisableWebviewGpu={settings.setDisableWebviewGpu}
                    idleDestroyEnabled={settings.idleDestroyEnabled}
                    setIdleDestroyEnabled={settings.setIdleDestroyEnabled}
                    idleDestroySeconds={settings.idleDestroySeconds}
                    setIdleDestroySeconds={settings.setIdleDestroySeconds}
                    soundEnabled={settings.soundEnabled}
                    setSoundEnabled={settings.setSoundEnabled}
                    soundVolume={settings.soundVolume}
                    setSoundVolume={settings.setSoundVolume}
                    pasteSoundEnabled={settings.pasteSoundEnabled}
                    setPasteSoundEnabled={settings.setPasteSoundEnabled}
                    showSearchBox={settings.showSearchBox}
                    setShowSearchBox={settings.setShowSearchBox}
                    scrollTopButtonEnabled={settings.scrollTopButtonEnabled}
                    setScrollTopButtonEnabled={settings.setScrollTopButtonEnabled}
                    emojiPanelEnabled={settings.emojiPanelEnabled}
                    setEmojiPanelEnabled={settings.setEmojiPanelEnabled}
                    tagManagerEnabled={settings.tagManagerEnabled}
                    setTagManagerEnabled={settings.setTagManagerEnabled}
                    arrowKeySelection={settings.arrowKeySelection}
                    setArrowKeySelection={settings.setArrowKeySelection}
                    saveAppSetting={saveAppSetting}
                />
            </div>

            <div className="settings-group-slot" hidden={!shown("clipboard")}>
                <ClipboardSettingsGroup
                    t={t}
                    collapsed={isCollapsed("clipboard")}
                    onToggle={() => collapse("clipboard")}
                    LabelWithHint={LabelWithHint}
                    persistent={settings.persistent}
                    setPersistent={settings.setPersistent}
                    persistentLimitEnabled={settings.persistentLimitEnabled}
                    setPersistentLimitEnabled={settings.setPersistentLimitEnabled}
                    persistentLimit={settings.persistentLimit}
                    setPersistentLimit={settings.setPersistentLimit}
                    saveAppSetting={saveAppSetting}
                    deduplicate={settings.deduplicate}
                    setDeduplicate={settings.setDeduplicate}
                    captureFiles={settings.captureFiles}
                    setCaptureFiles={settings.setCaptureFiles}
                    captureRichText={settings.captureRichText}
                    setCaptureRichText={settings.setCaptureRichText}
                    richTextSnapshotPreview={settings.richTextSnapshotPreview}
                    setRichTextSnapshotPreview={settings.setRichTextSnapshotPreview}
                    richTextSnapshotCacheLimit={settings.richTextSnapshotCacheLimit}
                    setRichTextSnapshotCacheLimit={settings.setRichTextSnapshotCacheLimit}
                    richPasteHotkey={settings.richPasteHotkey}
                    isRecordingRich={settings.isRecordingRich}
                    setIsRecordingRich={settings.setIsRecordingRich}
                    updateRichPasteHotkey={settings.updateRichPasteHotkey}
                    searchHotkey={settings.searchHotkey}
                    isRecordingSearch={settings.isRecordingSearch}
                    setIsRecordingSearch={settings.setIsRecordingSearch}
                    updateSearchHotkey={settings.updateSearchHotkey}
                    deleteAfterPaste={settings.deleteAfterPaste}
                    setDeleteAfterPaste={settings.setDeleteAfterPaste}
                    moveToTopAfterPaste={settings.moveToTopAfterPaste}
                    setMoveToTopAfterPaste={settings.setMoveToTopAfterPaste}
                    pasteMethod={settings.pasteMethod}
                    setPasteMethod={settings.setPasteMethod}
                    sequentialMode={settings.sequentialMode}
                    pasteQueueCount={settings.pasteQueueCount}
                    clearPasteQueue={settings.clearPasteQueue}
                    setSequentialModeState={settings.setSequentialModeState}
                    sequentialHotkey={settings.sequentialHotkey}
                    isRecordingSequential={settings.isRecordingSequential}
                    setIsRecordingSequential={settings.setIsRecordingSequential}
                    updateSequentialHotkey={settings.updateSequentialHotkey}
                    checkHotkeyConflict={settings.checkHotkeyConflict}
                    privacyProtection={settings.privacyProtection}
                    setPrivacyProtection={settings.setPrivacyProtection}
                    privacyProtectionKinds={settings.privacyProtectionKinds}
                    setPrivacyProtectionKinds={settings.setPrivacyProtectionKinds}
                    privacyProtectionCustomRules={settings.privacyProtectionCustomRules}
                    setPrivacyProtectionCustomRules={settings.setPrivacyProtectionCustomRules}
                    privacyKindsOpen={privacyKindsOpen}
                    setPrivacyKindsOpen={setPrivacyKindsOpen}
                    privacyRulesOpen={privacyRulesOpen}
                    setPrivacyRulesOpen={setPrivacyRulesOpen}
                    registryWinVEnabled={settings.registryWinVEnabled}
                    setRegistryWinVEnabled={settings.setRegistryWinVEnabled}
                    isRecording={settings.isRecording}
                    setIsRecording={settings.setIsRecording}
                    mainHotkeys={settings.mainHotkeys}
                    updateHotkey={settings.updateHotkey}
                    addMainHotkey={settings.addMainHotkey}
                    removeMainHotkey={settings.removeMainHotkey}
                    hotkey={settings.hotkey}
                    appSettings={settings.appSettings}
                    theme={settings.theme}
                    colorMode={settings.colorMode}
                />
            </div>

            <div className="settings-group-slot" hidden={!shown("appearance")}>
                <AppearanceSettingsGroup
                    t={t}
                    collapsed={isCollapsed("appearance")}
                    onToggle={() => collapse("appearance")}
                    LabelWithHint={LabelWithHint}
                    theme={settings.theme}
                    setTheme={settings.setTheme}
                    colorMode={settings.colorMode}
                    setColorMode={settings.setColorMode}
                    language={settings.language}
                    setLanguage={settings.setLanguage}
                    showAppBorder={settings.showAppBorder}
                    setShowAppBorder={settings.setShowAppBorder}
                    compactMode={settings.compactMode}
                    setCompactMode={settings.setCompactMode}
                    clipboardItemFontSize={settings.clipboardItemFontSize}
                    setClipboardItemFontSize={settings.setClipboardItemFontSize}
                    clipboardTagFontSize={settings.clipboardTagFontSize}
                    setClipboardTagFontSize={settings.setClipboardTagFontSize}
                    fontMain={settings.fontMain}
                    setFontMain={settings.setFontMain}
                    fontMono={settings.fontMono}
                    setFontMono={settings.setFontMono}
                    customBackground={settings.customBackground}
                    setCustomBackground={settings.setCustomBackground}
                    customBackgroundOpacity={settings.customBackgroundOpacity}
                    setCustomBackgroundOpacity={settings.setCustomBackgroundOpacity}
                    surfaceOpacity={settings.surfaceOpacity}
                    setSurfaceOpacity={settings.setSurfaceOpacity}
                    saveAppSetting={saveAppSetting}
                />
            </div>

            <div className="settings-group-slot" hidden={!shown("default_apps")}>
                <DefaultAppsSettingsGroup
                    t={t}
                    collapsed={isCollapsed("default_apps")}
                    onToggle={() => collapse("default_apps")}
                    installedApps={settings.installedApps}
                    appSettings={settings.appSettings}
                    defaultApps={settings.defaultApps}
                    saveAppSetting={saveAppSetting}
                />
            </div>

            <div className="settings-group-slot" hidden={!shown("data")}>
                <DataSettingsGroup
                    t={t}
                    collapsed={isCollapsed("data")}
                    onToggle={() => collapse("data")}
                    dataPath={settings.dataPath}
                />
            </div>

            <div className="settings-group-slot" hidden={!shown("tools")}>
                <ToolsSettingsGroup
                    t={t}
                    collapsed={isCollapsed("tools")}
                    onToggle={() => collapse("tools")}
                    LabelWithHint={LabelWithHint}
                    screenshotEnabled={screenshotEnabled}
                    setScreenshotEnabled={setScreenshotEnabled}
                    screenshotHotkey={screenshotHotkey}
                    screenshotFullHotkey={settings.appSettings?.["app.screenshot_full_hotkey"] || "Ctrl+Shift+F"}
                    quickPasteEnabled={quickPasteEnabled}
                    setQuickPasteEnabled={setQuickPasteEnabled}
                    quickPasteHotkey={settings.appSettings?.["app.quick_paste_hotkey"] || "Ctrl+Shift+V"}
                    quickPasteRenderer={settings.appSettings?.["app.quick_paste_renderer"] || "tauri"}
                    ocrEnabled={ocrEnabled}
                    setOcrEnabled={setOcrEnabled}
                    saveAppSetting={saveAppSetting}
                />
            </div>
            <div className="settings-group-slot" hidden={!shown("about")}>
                <AboutSettingsGroup
                    t={t}
                    collapsed={isCollapsed("about")}
                    collapsible={!split}
                    onToggle={() => collapse("about")}
                />
            </div>
        </>
    );
};

export default SettingsGroups;
