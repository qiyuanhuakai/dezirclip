import { useSyncExternalStore } from "react";
import { getPrivacyViewState, subscribePrivacyState } from "../lib/privacySession";

export function usePrivacySession() {
  return useSyncExternalStore(subscribePrivacyState, getPrivacyViewState, getPrivacyViewState);
}
