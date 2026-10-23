import type { PrivacyViewState } from "./privacySession";

export interface PrivacyMetadataIntent {
  epoch: number;
  sessionActive: boolean;
  isCurrent?: () => boolean;
}

export function capturePrivacyMetadataIntent(state: PrivacyViewState): PrivacyMetadataIntent | null {
  if (state.phase !== "ready" || !state.snapshot || state.pending) return null;
  return { epoch: state.snapshot.privacy_epoch, sessionActive: state.snapshot.session.active };
}

export const needsMetadataPersistenceConfirmation = (intent: PrivacyMetadataIntent, id: number, persists: boolean): boolean =>
  id < 0 && persists && intent.sessionActive;

export function privacyMetadataWriteArgs(intent: PrivacyMetadataIntent, current: PrivacyViewState, confirmed: boolean) {
  if (current.phase !== "ready" || current.pending || !current.snapshot
    || current.snapshot.privacy_epoch !== intent.epoch
    || current.snapshot.session.active !== intent.sessionActive || intent.isCurrent?.() === false) {
    throw new Error("PrivacyEpochChanged");
  }
  return { expectedPrivacyEpoch: intent.epoch, confirmPersistence: confirmed, allowSensitive: false };
}
