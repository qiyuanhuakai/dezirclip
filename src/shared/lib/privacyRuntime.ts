import type { ClipboardEntry } from "../types";

export interface PrivacyRuntimeSnapshot {
  history_revision: number;
  privacy_epoch: number;
  session: { active: boolean; expires_at_ms: number | null };
}

const record = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);
const counter = (value: unknown): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0;

export function parsePrivacySnapshot(value: unknown): PrivacyRuntimeSnapshot {
  if (!record(value) || !counter(value.history_revision) || !counter(value.privacy_epoch)
    || !record(value.session) || typeof value.session.active !== "boolean"
    || (value.session.expires_at_ms !== null && !counter(value.session.expires_at_ms))) {
    throw new Error("PrivacyStateUnavailable");
  }
  return {
    history_revision: value.history_revision,
    privacy_epoch: value.privacy_epoch,
    session: { active: value.session.active, expires_at_ms: value.session.expires_at_ms },
  };
}

export const isSensitiveEntry = (entry: Pick<ClipboardEntry, "tags">): boolean =>
  !entry.tags || entry.tags.some(tag => ["sensitive", "password", "密码"].includes(tag.trim().toLowerCase()));

export const concealSensitiveEntry = (entry: ClipboardEntry): ClipboardEntry => ({
  ...entry, content: "", preview: "••••••", html_content: undefined, ocr_text: null,
  ocr_status: null, file_preview_exists: false,
});

export function createPrivacyFence() {
  const identity = Symbol("privacy scope");
  let generation = 0;
  return {
    capture: () => ({ identity, generation }),
    current: () => generation,
    revoke: () => { generation += 1; },
    isCurrent: (token: { identity: symbol; generation: number }) =>
      token.identity === identity && token.generation === generation,
  };
}
