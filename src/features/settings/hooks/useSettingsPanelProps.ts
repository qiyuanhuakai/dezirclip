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

/**
 * The slice of the app state the settings feature actually reads.
 *
 * This used to be the whole `AppState` spread flat, with three keys destructured
 * away and an `as SettingsPanelProps` cast on the result. The cast is what made
 * the leak invisible: `SettingsPanelProps` declares what the settings groups
 * read, and the spread handed them thirty-seven more keys on top -- `search`,
 * `history`, `selectedIndex`, `currentOffset`, `revealedIds`, the type filter,
 * the panel visibility flags. `memo` compares every prop it is given, including
 * keys the component never reads, so moving the selection changed
 * `selectedIndex` and the boundary could not bail.
 *
 * The panel is mounted for the whole life of the app and hidden with
 * `display: none`, so that meant the entire settings subtree -- roughly a
 * hundred kilobytes of components across the general, clipboard, appearance and
 * data groups -- re-rendered while nobody could see it, on every keystroke in
 * the search box, every selection move and every clipboard capture.
 *
 * `Pick` is the fix. The compiler rejects a key that does not exist on
 * `AppState`, the returned object has to satisfy `SettingsPanelProps`, and
 * anything the settings feature does not declare cannot get in. Narrowing to
 * declared fields is not a behaviour change: the groups are already typed
 * against `SettingsPanelProps`, so they could only ever read these.
 */
const SETTINGS_STATE_KEYS = [
  "appSettings",
  "arrowKeySelection",
  "autoStart",
  "captureFiles",
  "captureRichText",
  "clipboardItemFontSize",
  "clipboardTagFontSize",
  "collapsedGroups",
  "compactMode",
  "customBackground",
  "customBackgroundOpacity",
  "dataPath",
  "defaultApps",
  "deduplicate",
  "deleteAfterPaste",
  "disableWebviewGpu",
  "edgeDocking",
  "emojiPanelEnabled",
  "fontMain",
  "fontMono",
  "followMouse",
  "hideTrayIcon",
  "hotkey",
  "idleDestroyEnabled",
  "idleDestroySeconds",
  "installedApps",
  "isRecording",
  "isRecordingRich",
  "isRecordingSearch",
  "isRecordingSequential",
  "moveToTopAfterPaste",
  "pasteMethod",
  "pasteSoundEnabled",
  "persistent",
  "persistentLimit",
  "persistentLimitEnabled",
  "privacyProtection",
  "privacyProtectionCustomRules",
  "privacyProtectionKinds",
  "registryWinVEnabled",
  "richPasteHotkey",
  "richTextSnapshotPreview",
  "scrollTopButtonEnabled",
  "searchHotkey",
  "sequentialHotkey",
  "sequentialMode",
  "showAppBorder",
  "showSearchBox",
  "silentStart",
  "soundEnabled",
  "soundVolume",
  "surfaceOpacity",
  "tagManagerEnabled",
  "setArrowKeySelection",
  "setAutoStart",
  "setCaptureFiles",
  "setCaptureRichText",
  "setClipboardItemFontSize",
  "setClipboardTagFontSize",
  "setCompactMode",
  "setCustomBackground",
  "setCustomBackgroundOpacity",
  "setDeleteAfterPaste",
  "setDeduplicate",
  "setDisableWebviewGpu",
  "setEdgeDocking",
  "setEmojiPanelEnabled",
  "setFollowMouse",
  "setFontMain",
  "setFontMono",
  "setHideTrayIcon",
  "setIdleDestroyEnabled",
  "setIdleDestroySeconds",
  "setIsRecording",
  "setIsRecordingRich",
  "setIsRecordingSearch",
  "setIsRecordingSequential",
  "setMoveToTopAfterPaste",
  "setPasteMethod",
  "setPasteSoundEnabled",
  "setPersistent",
  "setPersistentLimit",
  "setPersistentLimitEnabled",
  "setPrivacyProtection",
  "setPrivacyProtectionCustomRules",
  "setPrivacyProtectionKinds",
  "setRegistryWinVEnabled",
  "setRichPasteHotkey",
  "setRichTextSnapshotPreview",
  "setScrollTopButtonEnabled",
  "setSearchHotkey",
  "setSequentialHotkey",
  "setSequentialModeState",
  "setShowAppBorder",
  "setShowSearchBox",
  "setSilentStart",
  "setSoundEnabled",
  "setSoundVolume",
  "setSurfaceOpacity",
  "setTagManagerEnabled",
  "setTheme",
  "setColorMode",
  "setLanguage",
] as const satisfies ReadonlyArray<keyof AppState>;

/**
 * Derived from the array below rather than declared beside it.
 *
 * These used to be two independent lists that had to be kept in step by hand:
 * a union of key names, and a runtime array annotated
 * `ReadonlyArray<keyof SettingsStateSlice>`. That annotation only checks the
 * other direction -- every name in the array is a real key. It says nothing
 * about a name in the union being *absent* from the array, and that is the
 * direction that bites: `pickKeys` returned `Pick<AppState, keyof
 * SettingsStateSlice>`, so its type promised every field while the object it
 * actually built was missing one. The compiler could not see the gap, and the
 * panel would have received `undefined` for a field it reads.
 *
 * Taking the union from the array makes the return type describe what
 * `pickKeys` really produces, so a dropped key now fails the object literal
 * against `SettingsPanelProps` at build time. A name that is not a key of
 * `AppState` still fails, on the `satisfies` clause.
 *
 * AGENTS.md forbids new tests on existing UI hooks, so the exhaustiveness
 * this review asked for is enforced by the type system instead of by a test.
 */
type SettingsStateSlice = Pick<AppState, (typeof SETTINGS_STATE_KEYS)[number]>;


const pickKeys = <T, K extends keyof T>(source: T, keys: ReadonlyArray<K>): Pick<T, K> => {
  const out = {} as Pick<T, K>;
  for (const key of keys) out[key] = source[key];
  return out;
};

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
  handleResetSettings,
  toggleGroup,
  state
}: UseSettingsPanelPropsOptions): SettingsPanelProps => {
  // Annotated, so the spread below is checked against the type the array
  // actually produces. A name missing from `SETTINGS_STATE_KEYS` leaves this
  // literal without that field and fails here, rather than reaching the panel
  // as `undefined`.
  const stateSlice: SettingsStateSlice = pickKeys(state, SETTINGS_STATE_KEYS);
  return {
    ...stateSlice,
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
    handleResetSettings,
    toggleGroup
  };
};
