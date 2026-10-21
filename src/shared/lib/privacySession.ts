import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { createPrivacyFence, parsePrivacySnapshot } from "./privacyRuntime";
import type { PrivacyRuntimeSnapshot } from "./privacyRuntime";
import type { ClipboardEntry } from "../types";
import { entryInstanceArgs } from "./historyIdentity";

export interface PrivacyViewState {
  phase: "loading" | "ready" | "unavailable";
  snapshot: PrivacyRuntimeSnapshot | null;
  pending: boolean;
  generation: number;
}
const fence = createPrivacyFence();
const reveals = new WeakMap<ClipboardEntry, ReturnType<typeof fence.capture>>();
let model: PrivacyViewState = { phase: "loading", snapshot: null, pending: false, generation: 0 };
const observers = new Set<() => void>();
const invalidators = new Set<() => void>();
let stopEvents: (() => void) | null = null;
let lifecycle = 0;
let expiryTimer: ReturnType<typeof setTimeout> | null = null;
let refreshRequest: Promise<void> | null = null;
let viewSuspended = false;

export function registerPrivacyReveal(entry: ClipboardEntry): void {
  if (model.phase !== "ready") throw new Error("PrivacyStateUnavailable");
  reveals.set(entry, fence.capture());
}
export function forgetPrivacyReveal(entry: ClipboardEntry): void { reveals.delete(entry); }
export function privacyRevealEpoch(entry: ClipboardEntry): number | null {
  const grant = reveals.get(entry);
  return model.phase === "ready" && grant && fence.isCurrent(grant)
    ? model.snapshot?.privacy_epoch ?? null : null;
}

export const getPrivacyViewState = () => model;
const publish = (next: Partial<PrivacyViewState>) => {
  model = { ...model, ...next, generation: fence.current() };
  observers.forEach(observer => observer());
};
const revoke = () => {
  fence.revoke();
  invalidators.forEach(invalidate => { try { invalidate(); } catch { /* The request fence remains revoked. */ } });
};
export function subscribePrivacyInvalidation(invalidate: () => void): () => void {
  invalidators.add(invalidate);
  return () => { invalidators.delete(invalidate); };
}
const scheduleExpiry = () => {
  if (expiryTimer !== null) clearTimeout(expiryTimer);
  expiryTimer = null;
  const session = model.snapshot?.session;
  if (model.phase === "ready" && session?.active && session.expires_at_ms !== null) {
    expiryTimer = setTimeout(() => { void refreshPrivacyState(); },
      Math.min(2_147_000_000, Math.max(1, session.expires_at_ms - Date.now() + 50)));
  }
};
const accept = (value: unknown) => {
  const snapshot = parsePrivacySnapshot(value);
  publish({ phase: "ready", snapshot });
  scheduleExpiry();
};

export async function refreshPrivacyState(): Promise<void> {
  revoke();
  publish({ phase: "loading" });
  if (viewSuspended) return;
  if (refreshRequest) return refreshRequest;
  refreshRequest = (async () => {
    for (;;) {
      if (viewSuspended) return;
      const token = fence.capture();
      try {
        const value = await invoke<unknown>("get_privacy_status");
        if (fence.isCurrent(token)) { accept(value); return; }
      } catch {
        if (fence.isCurrent(token)) { publish({ phase: "unavailable", snapshot: null }); return; }
      }
      if (observers.size === 0) return;
    }
  })().finally(() => { refreshRequest = null; });
  return refreshRequest;
}

const onFocus = () => { viewSuspended = false; void refreshPrivacyState(); };
const onHide = () => {
  viewSuspended = true;
  if (expiryTimer !== null) clearTimeout(expiryTimer);
  expiryTimer = null;
  revoke();
  publish({ phase: "loading" });
};
const onVisibility = () => { if (document.hidden) onHide(); else onFocus(); };
export function subscribePrivacyState(observer: () => void): () => void {
  observers.add(observer);
  if (observers.size === 1) {
    const started = ++lifecycle;
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onHide);
    window.addEventListener("pagehide", onHide);
    document.addEventListener("visibilitychange", onVisibility);
    const subscriptions = [listen<unknown>("privacy-state-changed", () => {
      if (started === lifecycle) void refreshPrivacyState();
    }), listen("privacy-view-hidden", () => { if (started === lifecycle) onHide(); }),
      listen("privacy-view-shown", () => { if (started === lifecycle) onFocus(); })];
    void Promise.all(subscriptions).then(stops => {
      const stop = () => { stops.forEach(unlisten => unlisten()); };
      if (started !== lifecycle) { stop(); return; }
      stopEvents = stop;
      void refreshPrivacyState();
    }, () => {
      subscriptions.forEach(subscription => { void subscription.then(stop => stop(), () => undefined); });
      if (started === lifecycle) {
        revoke();
        publish({ phase: "unavailable", snapshot: null });
      }
    });
  }
  return () => {
    observers.delete(observer);
    if (observers.size === 0) {
      lifecycle += 1;
      stopEvents?.();
      stopEvents = null;
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onHide);
      window.removeEventListener("pagehide", onHide);
      document.removeEventListener("visibilitychange", onVisibility);
      if (expiryTimer !== null) clearTimeout(expiryTimer);
      expiryTimer = null;
      revoke();
      viewSuspended = false;
      model = { phase: "loading", snapshot: null, pending: false, generation: fence.current() };
    }
  };
}

export async function setPrivacySession(enabled: boolean, durationMs: number | null = null): Promise<void> {
  if (model.pending) throw new Error("PrivacyRequestPending");
  revoke();
  publish({ phase: "loading", pending: true });
  try {
    await invoke<unknown>("set_privacy_session", { enabled, durationMs });
    await refreshPrivacyState();
    if (model.phase !== "ready" || model.snapshot?.session.active !== enabled) {
      throw new Error("PrivacyStateUnavailable");
    }
  } catch (error) {
    await refreshPrivacyState();
    throw error;
  } finally {
    publish({ pending: false });
  }
}

export async function invokePrivacyBound<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (model.phase !== "ready") throw new Error("PrivacyStateUnavailable");
  const token = fence.capture();
  const result = await invoke<T>(command, args);
  if (!fence.isCurrent(token) || model.phase !== "ready") throw new Error("PrivacyEpochChanged");
  return result;
}

export function beginPrivacyAccess() {
  const epoch = model.snapshot?.privacy_epoch;
  if (model.phase !== "ready" || epoch === undefined) return null;
  const token = fence.capture();
  return { epoch, isCurrent: () => fence.isCurrent(token) && model.phase === "ready" };
}

export function getPrivacyEntry(id: number, hostInstanceId?: string): Promise<ClipboardEntry> {
  const epoch = model.snapshot?.privacy_epoch;
  if (model.phase !== "ready" || epoch === undefined) return Promise.reject(new Error("PrivacyStateUnavailable"));
  return invokePrivacyBound<ClipboardEntry>("get_privacy_entry", { id, expectedEpoch: epoch, ...entryInstanceArgs(hostInstanceId) });
}

export type PrivacyFeedbackAction = "not_sensitive" | "exact_exception" | "ignore_source" | "protect_source";
export async function submitPrivacyFeedback(id: number, action: PrivacyFeedbackAction, hostInstanceId?: string): Promise<void> {
  const epoch = model.snapshot?.privacy_epoch;
  if (model.phase !== "ready" || epoch === undefined) throw new Error("PrivacyStateUnavailable");
  try {
    await invoke("privacy_feedback", { id, expectedEpoch: epoch, action, ...entryInstanceArgs(hostInstanceId) });
  } finally {
    await refreshPrivacyState();
  }
}
