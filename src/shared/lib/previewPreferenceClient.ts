import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { normalizePreviewOpacity, PREVIEW_KEYBOARD_KEY, PREVIEW_OPACITY_KEY, readPreviewPreferences } from "./previewPreferences";

const CHANGED_EVENT = "preview-preferences-changed";
let saveQueue = Promise.resolve();

export async function loadPreviewPreferences() {
  return readPreviewPreferences(await invoke<Record<string, string>>("get_settings"));
}

export function savePreviewPreference(key: "keyboardEnabled", value: boolean): Promise<void>;
export function savePreviewPreference(key: "opacity", value: number): Promise<void>;
export function savePreviewPreference(key: "keyboardEnabled" | "opacity", value: boolean | number): Promise<void> {
  const setting = key === "keyboardEnabled" ? PREVIEW_KEYBOARD_KEY : PREVIEW_OPACITY_KEY;
  if ((key === "keyboardEnabled" && typeof value !== "boolean") || (key === "opacity" && (typeof value !== "number" || !Number.isFinite(value)))) {
    return Promise.reject(new Error("InvalidPreviewPreference"));
  }
  const storedValue = key === "opacity" ? String(normalizePreviewOpacity(Number(value))) : String(value);
  const saved = saveQueue.then(async () => {
    await invoke("save_setting", { key: setting, value: storedValue });
    await emit(CHANGED_EVENT, { key: setting }).catch(error => {
      console.warn("[preview] Failed to notify other windows", error);
    });
  });
  saveQueue = saved.catch(() => undefined);
  return saved;
}

/** Events only invalidate metadata; every window reads committed settings itself. */
export async function subscribePreviewPreferenceChanges(changed: () => void): Promise<() => void> {
  const subscriptions = await Promise.allSettled([
    listen(CHANGED_EVENT, changed),
    listen("appearance-changed", changed)
  ]);
  const stops = subscriptions.flatMap(result => result.status === "fulfilled" ? [result.value] : []);
  if (subscriptions.some(result => result.status === "rejected")) {
    stops.forEach(stop => stop());
    throw new Error("PreviewPreferencesUnavailable");
  }
  return () => { stops.forEach(stop => stop()); };
}
