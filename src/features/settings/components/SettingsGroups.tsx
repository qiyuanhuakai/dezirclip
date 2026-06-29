import type { ComponentType } from "react";
import GeneralSettingsGroup from "./groups/GeneralSettingsGroup";
import ClipboardSettingsGroup from "./groups/ClipboardSettingsGroup";
import AppearanceSettingsGroup from "./groups/AppearanceSettingsGroup";
import DefaultAppsSettingsGroup from "./groups/DefaultAppsSettingsGroup";
import DataSettingsGroup from "./groups/DataSettingsGroup";
import ToolsSettingsGroup from "./groups/ToolsSettingsGroup";
import type { LabelWithHintProps, SettingsPanelProps } from "./SettingsPanel.types";

interface SettingsGroupsProps {
    readonly settings: SettingsPanelProps;
    readonly LabelWithHint: ComponentType<LabelWithHintProps>;
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

    return (
        <>
            <GeneralSettingsGroup
                t={t}
                collapsed={collapsedGroups["general"]}
                onToggle={() => toggleGroup("general")}
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

            <ClipboardSettingsGroup
                t={t}
                collapsed={collapsedGroups["clipboard"]}
                onToggle={() => toggleGroup("clipboard")}
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

            <AppearanceSettingsGroup
                t={t}
                collapsed={collapsedGroups["appearance"]}
                onToggle={() => toggleGroup("appearance")}
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

            <DefaultAppsSettingsGroup
                t={t}
                collapsed={collapsedGroups["default_apps"]}
                onToggle={() => toggleGroup("default_apps")}
                installedApps={settings.installedApps}
                appSettings={settings.appSettings}
                defaultApps={settings.defaultApps}
                saveAppSetting={saveAppSetting}
            />

            <DataSettingsGroup
                t={t}
                collapsed={collapsedGroups["data"]}
                onToggle={() => toggleGroup("data")}
                dataPath={settings.dataPath}
            />

            <ToolsSettingsGroup
                t={t}
                collapsed={collapsedGroups["tools"]}
                onToggle={() => toggleGroup("tools")}
                LabelWithHint={LabelWithHint}
                screenshotEnabled={screenshotEnabled}
                setScreenshotEnabled={setScreenshotEnabled}
                screenshotHotkey={screenshotHotkey}
                quickPasteEnabled={quickPasteEnabled}
                setQuickPasteEnabled={setQuickPasteEnabled}
                quickPasteHotkey={settings.appSettings?.["app.quick_paste_hotkey"] || "Ctrl+Shift+V"}
                ocrEnabled={ocrEnabled}
                setOcrEnabled={setOcrEnabled}
                saveAppSetting={saveAppSetting}
            />
        </>
    );
};

export default SettingsGroups;
