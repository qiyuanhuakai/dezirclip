import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface UseNavigationSyncOptions {
  showSettings: boolean;
  showTagManager: boolean;
  showEmojiPanel: boolean;
  searchTagPickerOpen: boolean;
}

export const useNavigationSync = ({
  showSettings,
  showTagManager,
  showEmojiPanel,
  searchTagPickerOpen
}: UseNavigationSyncOptions) => {
  useEffect(() => {
    const shouldDisableNavigation = showSettings || showTagManager || showEmojiPanel || searchTagPickerOpen;
    if (shouldDisableNavigation) {
      invoke("set_navigation_enabled", { enabled: false }).catch(console.error);
      return;
    }

    // Only enable global navigation when the window is actually visible.
    let disposed = false;
    getCurrentWindow()
      .isVisible()
      .then((visible) => {
        if (disposed) return;
        invoke("set_navigation_enabled", { enabled: visible }).catch(console.error);
      })
      .catch(() => {
        if (disposed) return;
        invoke("set_navigation_enabled", { enabled: false }).catch(console.error);
      });
    return () => { disposed = true; };
  }, [showSettings, showTagManager, showEmojiPanel, searchTagPickerOpen]);
};
