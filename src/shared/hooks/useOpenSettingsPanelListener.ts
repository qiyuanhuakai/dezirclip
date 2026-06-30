import { useEffect } from "react";
import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";

interface UseOpenSettingsPanelListenerOptions {
  readonly setShowSettings: (val: boolean) => void;
  readonly setShowTagManager: (val: boolean) => void;
  readonly setShowEmojiPanel: (val: boolean) => void;
}

export const useOpenSettingsPanelListener = ({
  setShowSettings,
  setShowTagManager,
  setShowEmojiPanel
}: UseOpenSettingsPanelListenerOptions) => {
  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | null = null;

    listen("open-settings-panel", () => {
      setShowTagManager(false);
      setShowEmojiPanel(false);
      setShowSettings(true);
      void emit("open-settings-panel-consumed");
    }).then((off) => {
      if (disposed) {
        off();
        return;
      }

      unlisten = off;
      void emit("settings-panel-listener-ready");
    }).catch(console.error);

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [setShowSettings, setShowTagManager, setShowEmojiPanel]);
};
