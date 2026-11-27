import type { PrivacyRuntimeSnapshot } from "./privacyRuntime";

export interface PrivacyViewState {
  phase: "loading" | "ready" | "unavailable";
  snapshot: PrivacyRuntimeSnapshot | null;
  pending: boolean;
  generation: number;
}

// The session owns updates and invalidates its request fences before publishing.
export let privacyViewState: PrivacyViewState = {
  phase: "loading",
  snapshot: null,
  pending: false,
  generation: 0,
};
export const getPrivacyViewState = () => privacyViewState;
export function setPrivacyViewState(next: PrivacyViewState) {
  privacyViewState = next;
}
