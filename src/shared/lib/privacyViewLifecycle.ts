import type { PrivacyRuntimeSnapshot } from "./privacyRuntime";
import { isPinnedPreviewLabel } from "./previewWindowLabels";

export type PrivacyInvalidationScope = "all" | "reveal";

export function samePrivacyAuthority(previous: PrivacyRuntimeSnapshot, next: PrivacyRuntimeSnapshot): boolean {
  return previous.privacy_epoch === next.privacy_epoch
    && previous.session.active === next.session.active
    && previous.session.expires_at_ms === next.session.expires_at_ms;
}

export function privacyBlurScope(visible: boolean, focused: boolean, ownedWindowFocused: boolean): PrivacyInvalidationScope | null {
  if (!visible) return "all";
  return focused || ownedWindowFocused ? null : "reveal";
}

export function sharesPrivacyFocusLifecycle(label: string): boolean {
  return label === "main" || label === "compact-preview" || label === "quick-paste" || isPinnedPreviewLabel(label);
}
