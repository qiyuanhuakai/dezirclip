import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { getAllWindows, getCurrentWindow } from "@tauri-apps/api/window";
import { createPrivacyFence, parsePrivacySnapshot } from "./privacyRuntime";
import type { PrivacyRuntimeSnapshot } from "./privacyRuntime";
import type { ClipboardEntry } from "../types";
import { entryInstanceArgs } from "./historyIdentity";
import { privacyBlurScope, samePrivacyAuthority } from "./privacyViewLifecycle";
import type { PrivacyInvalidationScope } from "./privacyViewLifecycle";
export type { PrivacyInvalidationScope } from "./privacyViewLifecycle";

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
const invalidators = new Set<(scope: PrivacyInvalidationScope) => void>();
let stopEvents: (() => void) | null = null;
let lifecycle = 0;
let expiryTimer: ReturnType<typeof setTimeout> | null = null;
let refreshRequest: Promise<void> | null = null;
let activationRequest: Promise<void> | null = null;
let viewSuspended = false;
let visibilityRevision = 0;
let activeView = true;
let nativeBlurPending = false;
const ownedWindowLabels = ["main", "compact-preview", "quick-paste", "region-select"];

export function registerPrivacyReveal(entry: ClipboardEntry): void {
  if (model.phase !== "ready" || !activeView) throw new Error("PrivacyStateUnavailable");
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
const revoke = (scope: PrivacyInvalidationScope = "all") => {
  fence.revoke();
  invalidators.forEach(invalidate => { try { invalidate(scope); } catch { /* The request fence remains revoked. */ } });
};
export function subscribePrivacyInvalidation(invalidate: (scope: PrivacyInvalidationScope) => void): () => void {
  invalidators.add(invalidate);
  return () => { invalidators.delete(invalidate); };
}
const scheduleExpiry = () => {
  if (expiryTimer !== null) clearTimeout(expiryTimer);
  expiryTimer = null;
  const session = model.snapshot?.session;
  if (model.phase === "ready" && session?.active && session.expires_at_ms !== null) {
    expiryTimer = setTimeout(() => { invalidatePrivacyState(); void refreshPrivacyState(); },
      Math.min(2_147_000_000, Math.max(1, session.expires_at_ms - Date.now() + 50)));
  }
};
const accept = (value: unknown) => {
  const snapshot = parsePrivacySnapshot(value);
  if (model.phase === "ready" && model.snapshot && !samePrivacyAuthority(model.snapshot, snapshot)) revoke();
  publish({ phase: "ready", snapshot });
  scheduleExpiry();
};

export async function refreshPrivacyState(): Promise<void> {
  if (viewSuspended) return;
  if (model.phase !== "ready") publish({ phase: "loading" });
  if (refreshRequest) return refreshRequest;
  refreshRequest = (async () => {
    for (;;) {
      if (viewSuspended) return;
      const token = fence.capture();
      try {
        const value = await invoke<unknown>("get_privacy_status");
        if (fence.isCurrent(token)) { accept(value); return; }
      } catch {
        if (fence.isCurrent(token)) { revoke(); publish({ phase: "unavailable", snapshot: null }); return; }
      }
      if (observers.size === 0) return;
    }
  })().finally(() => { refreshRequest = null; });
  return refreshRequest;
}

const onFocus = () => {
  if (nativeBlurPending) { nativeBlurPending = false; revokeApplicationReveals(); }
  const revision = ++visibilityRevision;
  if (document.hidden) { onHide(); return; }
  if (viewSuspended) {
    const started = lifecycle;
    void getCurrentWindow().isVisible().then(visible => {
      if (started !== lifecycle || revision !== visibilityRevision || !visible || document.hidden) return;
      viewSuspended = false;
      activeView = true;
      void refreshPrivacyState();
    }, () => undefined);
    return;
  }
  viewSuspended = false;
  activeView = true;
  void refreshPrivacyState();
};
const onHide = () => {
  nativeBlurPending = false;
  visibilityRevision += 1;
  viewSuspended = true;
  activeView = false;
  if (expiryTimer !== null) clearTimeout(expiryTimer);
  expiryTimer = null;
  revoke();
  publish({ phase: "loading" });
};
const revokeApplicationReveals = () => {
  activeView = false;
  revoke("reveal");
  publish({});
  void emit("privacy-application-blurred", { source: getCurrentWindow().label }).catch(() => undefined);
};
const onBlur = (event?: Event | boolean) => {
  const nativeBlur = event === true;
  if (nativeBlur) nativeBlurPending = true;
  const revision = nativeBlur ? ++visibilityRevision : visibilityRevision;
  const started = lifecycle;
  const current = getCurrentWindow();
  void (async () => {
    const visible = await current.isVisible();
    const focused = nativeBlur && visible && !document.hidden ? await current.isFocused() : false;
    const windows = nativeBlur && visible && !focused && !document.hidden ? await getAllWindows() : [];
    const ownedFocused = (await Promise.all(windows.filter(window => window.label !== current.label
      && ownedWindowLabels.includes(window.label))
      .map(window => window.isFocused().catch(() => false)))).some(Boolean);
    return { visible, focused, ownedFocused };
  })().then(({ visible, focused, ownedFocused }) => {
      if (started !== lifecycle || revision !== visibilityRevision) return;
      if (nativeBlur) nativeBlurPending = false;
      const scope = privacyBlurScope(visible && !document.hidden, focused || !nativeBlur, ownedFocused);
      if (scope === "all") onHide();
      else if (scope === "reveal") revokeApplicationReveals();
    }, () => {
      if (started === lifecycle && revision === visibilityRevision) {
        if (nativeBlur) { nativeBlurPending = false; revokeApplicationReveals(); }
        else onHide();
      }
    });
};
const invalidatePrivacyState = () => { revoke(); publish({ phase: "loading" }); };
const onVisibility = () => { if (document.hidden) onHide(); else onFocus(); };
export function subscribePrivacyState(observer: () => void): () => void {
  observers.add(observer);
  if (observers.size === 1) {
    const started = ++lifecycle;
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    window.addEventListener("pagehide", onHide);
    document.addEventListener("visibilitychange", onVisibility);
    const subscriptions = [listen<unknown>("privacy-state-changed", () => {
      if (started === lifecycle) { invalidatePrivacyState(); void refreshPrivacyState(); }
    }), listen("privacy-view-hidden", () => { if (started === lifecycle) onHide(); }),
      listen("privacy-view-shown", () => { if (started === lifecycle) onFocus(); }),
      listen("clipboard-host-changed", () => {
        if (started === lifecycle) { invalidatePrivacyState(); void refreshPrivacyState(); }
      }), listen("clipboard-host-unavailable", () => {
        if (started === lifecycle) { revoke(); publish({ phase: "unavailable", snapshot: null }); }
      }),
      listen<{ source: string }>("privacy-application-blurred", event => {
        if (started !== lifecycle || !ownedWindowLabels.includes(event.payload?.source)
          || event.payload.source === getCurrentWindow().label) return;
        activeView = false;
        revoke("reveal");
        publish({});
      }),
      getCurrentWindow().onFocusChanged(({ payload: focused }) => {
        if (started === lifecycle) { if (focused) onFocus(); else onBlur(true); }
      })];
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
      visibilityRevision += 1;
      stopEvents?.();
      stopEvents = null;
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("pagehide", onHide);
      document.removeEventListener("visibilitychange", onVisibility);
      if (expiryTimer !== null) clearTimeout(expiryTimer);
      expiryTimer = null;
      revoke();
      viewSuspended = false;
      activeView = true;
      nativeBlurPending = false;
      activationRequest = null;
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
  if (model.phase !== "ready" || (command === "get_privacy_entry" && args?.reveal !== false && !activeView)) throw new Error("PrivacyStateUnavailable");
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

export async function getPrivacyEntry(id: number, hostInstanceId?: string): Promise<ClipboardEntry> {
  const epoch = model.snapshot?.privacy_epoch;
  if (model.phase !== "ready" || epoch === undefined) throw new Error("PrivacyStateUnavailable");
  const token = fence.capture();
  const current = getCurrentWindow();
  if (current.label === "main") {
    const focused = await current.isFocused();
    if (!fence.isCurrent(token)) throw new Error("PrivacyEpochChanged");
    if (!focused) {
      if (!activationRequest) {
        const request = refreshPrivacyAfterConfirmation(epoch).finally(() => {
          if (activationRequest === request) activationRequest = null;
        });
        activationRequest = request;
      }
      await activationRequest;
    } else if (!activeView) {
      activeView = true;
      await refreshPrivacyState();
    }
  }
  if (!fence.isCurrent(token)) throw new Error("PrivacyEpochChanged");
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

export async function refreshPrivacyAfterConfirmation(expectedEpoch?: number): Promise<void> {
  await invoke("activate_window_focus");
  await new Promise<void>((resolve, reject) => {
    const finish = (error?: Error) => {
      clearTimeout(timeout);
      observers.delete(check);
      window.removeEventListener("focus", check);
      if (error) reject(error); else resolve();
    };
    const check = () => {
      if (viewSuspended || document.hidden || !document.hasFocus() || model.phase !== "ready") return;
      finish(expectedEpoch === undefined || model.snapshot?.privacy_epoch === expectedEpoch ? undefined : new Error("PrivacyEpochChanged"));
    };
    const timeout = setTimeout(() => finish(new Error("PrivacyStateUnavailable")), 2000);
    observers.add(check);
    window.addEventListener("focus", check);
    void refreshPrivacyState();
    check();
  });
}
