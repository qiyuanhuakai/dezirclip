import { useEffect, useRef } from "react";
import { createCollapsedSettingsGroups } from "../../features/settings/utils/settingsLayout";
import { settingsNavigation } from "../lib/settingsNavigation";

interface UseSettingsPanelResetOptions {
  showSettings: boolean;
  setCollapsedGroups: (val: Record<string, boolean>) => void;
}

export const useSettingsPanelReset = ({
  showSettings,
  setCollapsedGroups
}: UseSettingsPanelResetOptions) => {
  const wasOpen = useRef(showSettings);
  useEffect(() => {
    if (wasOpen.current && !showSettings) {
      setCollapsedGroups(createCollapsedSettingsGroups());
      settingsNavigation.close();
    }
    wasOpen.current = showSettings;
  }, [showSettings, setCollapsedGroups]);
};
