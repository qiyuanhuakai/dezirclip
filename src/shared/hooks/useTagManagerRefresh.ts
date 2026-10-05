import { useEffect, useRef } from "react";

interface UseTagManagerRefreshOptions {
  showTagManager: boolean;
  settingsLoaded: boolean;
  persistentLimitEnabled: boolean;
  persistentLimit: number;
  fetchHistory: (reset?: boolean) => void;
}

export const useTagManagerRefresh = ({
  showTagManager,
  settingsLoaded,
  persistentLimitEnabled,
  persistentLimit,
  fetchHistory
}: UseTagManagerRefreshOptions) => {
  // The refetch exists to pick up whatever the tag manager changed while it
  // was open, so it belongs to the open -> closed transition. It also used to
  // fire the first time the settings finished loading, which added a duplicate
  // first-page query to every startup and every wake after the webview was
  // rebuilt.
  const wasOpenRef = useRef<boolean | null>(null);
  useEffect(() => {
    const wasOpen = wasOpenRef.current;
    wasOpenRef.current = showTagManager;
    if (!settingsLoaded) return;
    if (wasOpen !== true) return;
    fetchHistory(true);
  }, [showTagManager, settingsLoaded, persistentLimitEnabled, persistentLimit, fetchHistory]);
};
