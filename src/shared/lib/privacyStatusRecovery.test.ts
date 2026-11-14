import { describe, expect, it } from "vitest";
import { privacyStatusRecoveryDelay } from "./privacyStatusRecovery";

describe("privacy status recovery", () => {
  it("bounds retries of explicitly transient status reads", () => {
    const error = new Error("CallError: PrivacyStatusRetryable: remote-Busy");
    expect([0, 1, 2, 3].map(attempt => privacyStatusRecoveryDelay(error, attempt))).toEqual([500, 1000, 2000, null]);
    expect(privacyStatusRecoveryDelay("PrivacyStatusRetryable: eof", 0)).toBe(500);
    expect(privacyStatusRecoveryDelay(error, -1)).toBeNull();
  });
  it("does not automatically retry denied, malformed or unknown status", () => {
    for (const error of ["PermissionDenied", "PrivacyEpochChanged", "PrivacyStateUnavailable", "Busy", {}, null]) {
      expect(privacyStatusRecoveryDelay(error, 0)).toBeNull();
    }
  });
});
