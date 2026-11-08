import { useEffect, useRef } from "react";

interface UseSettingsSyncOptions {
  settingsLoaded: boolean;
  deduplicate: boolean;
  saveAppSetting: (type: string, value: string) => void;
  saveSetting: (key: string, value: string) => void;
  captureFiles: boolean;
  captureRichText: boolean;
  persistent: boolean;
  soundVolume: number;
  arrowKeySelection: boolean;
  setIsKeyboardMode: (val: boolean) => void;
  setSelectedIndex: (val: number) => void;
}

export const useSettingsSync = ({
  settingsLoaded,
  deduplicate,
  saveAppSetting,
  saveSetting,
  captureFiles,
  captureRichText,
  persistent,
  soundVolume,
  arrowKeySelection,
  setIsKeyboardMode,
  setSelectedIndex
}: UseSettingsSyncOptions) => {
  const previous = useRef<{ deduplicate: boolean; captureFiles: boolean; captureRichText: boolean;
    persistent: boolean; soundVolume: number; arrowKeySelection: boolean } | null>(null);
  useEffect(() => {
    if (!settingsLoaded) {
      previous.current = null;
      return;
    }
    const last = previous.current;
    previous.current = { deduplicate, captureFiles, captureRichText, persistent, soundVolume, arrowKeySelection };
    // The first loaded values came from the Host. Writing them back also wakes
    // every settings listener while a freshly opened window is restoring history.
    if (!last) return;
    if (last.deduplicate !== deduplicate) saveAppSetting("deduplicate", String(deduplicate));
    if (last.captureFiles !== captureFiles) saveSetting("app.capture_files", String(captureFiles));
    if (last.captureRichText !== captureRichText) saveSetting("app.capture_rich_text", String(captureRichText));
    if (last.persistent !== persistent) saveSetting("app.persistent", String(persistent));
    if (last.soundVolume !== soundVolume) saveAppSetting("sound_volume", String(soundVolume));
    if (last.arrowKeySelection !== arrowKeySelection) saveSetting("app.arrow_key_selection", String(arrowKeySelection));
  }, [settingsLoaded, deduplicate, captureFiles, captureRichText, persistent, soundVolume, arrowKeySelection, saveAppSetting, saveSetting]);

  useEffect(() => {
    if (!settingsLoaded) return;
    if (!arrowKeySelection) {
      setIsKeyboardMode(false);
      setSelectedIndex(0);
    }
  }, [arrowKeySelection, setIsKeyboardMode, setSelectedIndex, settingsLoaded]);
};
