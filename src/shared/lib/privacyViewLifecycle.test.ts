import { describe, expect, it } from "vitest";
import { privacyBlurScope, samePrivacyAuthority } from "./privacyViewLifecycle";

describe("privacy view lifecycle", () => {
  const state = { privacy_epoch: 7, history_revision: 2, session: { active: false, expires_at_ms: null } };
  it("does not revoke unchanged authority for status refresh or history metadata changes", () => {
    expect(samePrivacyAuthority(state, { ...state, history_revision: 3 })).toBe(true);
  });
  it("detects epoch and privacy session transitions", () => {
    expect(samePrivacyAuthority(state, { ...state, privacy_epoch: 8 })).toBe(false);
    expect(samePrivacyAuthority(state, { ...state, session: { active: true, expires_at_ms: null } })).toBe(false);
    expect(samePrivacyAuthority(state, { ...state, session: { active: false, expires_at_ms: 100 } })).toBe(false);
  });
  it("preserves a native-focused title bar and focused owned preview", () => {
    expect(privacyBlurScope(true, true, false)).toBeNull();
    expect(privacyBlurScope(true, false, true)).toBeNull();
  });
  it("revokes explicit reveals when a visible pinned window loses application focus", () => {
    expect(privacyBlurScope(true, false, false)).toBe("reveal");
  });
  it("fully revokes a hidden window regardless of stale focus values", () => {
    expect(privacyBlurScope(false, true, true)).toBe("all");
  });
});
