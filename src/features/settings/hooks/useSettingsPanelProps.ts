import type { AppState } from "../../app/types";
import type { Locale } from "../../../shared/types";
import type { SettingsPanelProps } from "../components/SettingsPanel.types";

interface UseSettingsPanelPropsOptions {
  t: (key: string) => string;
  theme: string;
  language: Locale;
  colorMode: string;
  mainHotkeys: string[];
  checkHotkeyConflict: (newHotkey: string, mode: "main" | "sequential" | "rich" | "search") => boolean;
  updateHotkey: (key: string) => void;
  addMainHotkey: (key: string, options?: { skipAvailabilityCheck?: boolean }) => Promise<boolean>;
  removeMainHotkey: (key: string) => Promise<boolean>;
  updateSequentialHotkey: (key: string) => void;
  updateRichPasteHotkey: (key: string) => void;
  updateSearchHotkey: (key: string) => void;
  saveAppSetting: (key: string, val: string) => void;
  saveSetting: (key: string, val: string) => void;
  handleResetSettings: () => void;
  toggleGroup: (group: string) => void;
  state: AppState;
}

export const useSettingsPanelProps = ({
  t,
  theme,
  language,
  colorMode,
  mainHotkeys,
  checkHotkeyConflict,
  updateHotkey,
  addMainHotkey,
  removeMainHotkey,
  updateSequentialHotkey,
  updateRichPasteHotkey,
  updateSearchHotkey,
  saveAppSetting,
  saveSetting,
  handleResetSettings,
  toggleGroup,
  state
}: UseSettingsPanelPropsOptions): SettingsPanelProps => {
  // The panel is rendered inside the settings view, so it is told when its own
  // visibility flips on every panel switch. Nothing in the settings feature
  // reads these three, and letting them through re-renders the whole settings
  // subtree each time, which is what stops the memo on the panel from bailing.
  const { showSettings, showTagManager, showEmojiPanel, ...panelState } = state;
  return {
    ...panelState,
    t,
    theme,
    language,
    colorMode,
    mainHotkeys,
    checkHotkeyConflict,
    updateHotkey,
    addMainHotkey,
    removeMainHotkey,
    updateSequentialHotkey,
    updateRichPasteHotkey,
    updateSearchHotkey,
    saveAppSetting,
    saveSetting,
    handleResetSettings,
    toggleGroup
  } as SettingsPanelProps;
};
