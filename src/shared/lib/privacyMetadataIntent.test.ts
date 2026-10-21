import { describe, expect, it } from "vitest";
import type { PrivacyViewState } from "./privacySession";
import { capturePrivacyMetadataIntent, needsMetadataPersistenceConfirmation, privacyMetadataWriteArgs } from "./privacyMetadataIntent";

const ready = (epoch: number, active: boolean): PrivacyViewState => ({
  phase: "ready", pending: false, generation: 1,
  snapshot: { history_revision: 10, privacy_epoch: epoch, session: { active, expires_at_ms: null } },
});

describe("privacy metadata write intent", () => {
  it("captures the original epoch without authorizing content reads", () => {
    const state = ready(7, true);
    const intent = capturePrivacyMetadataIntent(state)!;
    expect(intent).toEqual({ epoch: 7, sessionActive: true });
    expect(privacyMetadataWriteArgs(intent, state, true)).toEqual({
      expectedPrivacyEpoch: 7, confirmPersistence: true, allowSensitive: false,
    });
  });
  it("requires persistence confirmation only for an active-session RAM write", () => {
    for (const id of [-1, 5]) for (const active of [false, true]) for (const persists of [false, true]) {
      const intent = capturePrivacyMetadataIntent(ready(7, active))!;
      expect(needsMetadataPersistenceConfirmation(intent, id, persists)).toBe(id < 0 && active && persists);
    }
  });
  it("does not capture unavailable or pending state", () => {
    expect(capturePrivacyMetadataIntent({ ...ready(7, true), phase: "loading" })).toBeNull();
    expect(capturePrivacyMetadataIntent({ ...ready(7, true), pending: true })).toBeNull();
    expect(capturePrivacyMetadataIntent({ ...ready(7, true), snapshot: null })).toBeNull();
  });
  it("never substitutes a newer epoch after confirmation", () => {
    const intent = capturePrivacyMetadataIntent(ready(7, true))!;
    expect(() => privacyMetadataWriteArgs(intent, ready(8, true), true)).toThrow("PrivacyEpochChanged");
    expect(intent.epoch).toBe(7);
    expect(() => privacyMetadataWriteArgs(intent, ready(7, false), true)).toThrow("PrivacyEpochChanged");
  });
  it("rejects a source that no longer matches the original record", () => {
    const intent = { ...capturePrivacyMetadataIntent(ready(7, true))!, isCurrent: () => false };
    expect(() => privacyMetadataWriteArgs(intent, ready(7, true), true)).toThrow("PrivacyEpochChanged");
  });
  it("keeps metadata-only permission when no persistence confirmation is needed", () => {
    const state = ready(7, false);
    expect(privacyMetadataWriteArgs(capturePrivacyMetadataIntent(state)!, state, false)).toEqual({
      expectedPrivacyEpoch: 7, confirmPersistence: false, allowSensitive: false,
    });
  });
});
