import type { Event as DesktopEvent } from "@tauri-apps/api/event";
import { observeNativeAppearance } from "./appearance";
import type { RuntimeState, EventsResult } from "../protocol/wire";

const bus = new EventTarget();
let nextId = 0;
let watching = false;
let connected = false;
let generation = 0;
let settingsListeners = 0;
let pendingSettings = false;
let searchListeners = 0;
let pendingSearch = false;

async function receiveSearchRequest() {
  if (await window.mygo.call<boolean>("DesktopShell.TakeSearchRequest")) pendingSearch = true;
  if (searchListeners && pendingSearch) {
    pendingSearch = false;
    dispatch("focus-search-input");
  }
}

async function receiveSettingsRequest() {
  if (await window.mygo.call<boolean>("DesktopShell.TakeSettingsRequest")) pendingSettings = true;
  if (settingsListeners && pendingSettings) {
    pendingSettings = false;
    dispatch("open-settings-panel");
  }
}

function dispatch(name: string, payload?: unknown) {
  bus.dispatchEvent(new CustomEvent(name, { detail: payload }));
}

async function watchHost() {
  let previous: RuntimeState | undefined;
  let backoff = 250;
  for (;;) {
    try {
      const update = await window.mygo.call<EventsResult>("History.Changes", previous?.event_sequence ?? null);
      const recovered = !connected;
      connected = true;
      backoff = 250;
      const resync = recovered || update.resync_required || previous?.service_instance_id !== update.state.service_instance_id;
      const privacyChanged = previous?.privacy_epoch !== update.state.privacy_epoch;
      if (resync || privacyChanged) generation++;
      if (resync) dispatch("clipboard-host-changed", update.state.service_instance_id);
      if (privacyChanged) dispatch("privacy-state-changed");
      const events = update.events.filter(({ state }) => state.privacy_epoch === update.state.privacy_epoch
        && state.service_instance_id === update.state.service_instance_id);
      const refreshHistory = resync || privacyChanged || previous?.history_revision !== update.state.history_revision;
      const removedIds = events.flatMap(({ event, data }) => event === "EntryDeleted" && data.entry_id !== undefined ? [data.entry_id] : []);
      const onlyRemovals = !resync && !privacyChanged && previous !== undefined && removedIds.length > 0
        && update.state.history_revision - previous.history_revision === removedIds.length
        && !events.some(({ event }) => event === "EntryAdded" || event === "EntryUpdated" || event === "HistoryReset");
      let settingsChanged = resync || privacyChanged;
      for (const { event, data, state } of events) {
        if ((event === "EntryAdded" || event === "EntryUpdated") && data.entry_id !== undefined) {
          // A pending page refresh invalidates any summary requested by this event.
          if (!refreshHistory) {
            dispatch("clipboard-captured", { id: data.entry_id, host_instance_id: state.service_instance_id });
          }
          if (event === "EntryUpdated" && data.status && ["processing", "done", "failed", "unsupported"].includes(data.status)) {
            dispatch("ocr:complete", { item_id: data.entry_id, text: "", confidence: null, status: data.status });
          }
        } else if (event === "EntryDeleted" && !onlyRemovals) {
          dispatch("clipboard-removed", data.entry_id);
        } else if (event === "HistoryReset") {
          dispatch("tag-colors-updated");
        } else if (event === "ServiceStateChanged" && data.status && ["capture_settings_changed", "view_settings_changed", "custom_themes_changed", "PrivacyHotkeyChanged", "application_settings_reset"].includes(data.status)) {
          settingsChanged = true;
        } else if (event === "ServiceStateChanged" && data.status?.startsWith("paste_queue_changed:")) {
          settingsChanged = true;
          const count = Number(data.status.slice("paste_queue_changed:".length));
          if (Number.isSafeInteger(count) && count >= 0) dispatch("paste-queue-changed", count);
        }
      }
      if (settingsChanged) {
        dispatch("settings-changed");
        dispatch("preview-preferences-changed");
        void window.mygo.call("Preview.ReloadOpacity").catch(console.error);
      }
      if (refreshHistory) {
        dispatch("clipboard-changed", onlyRemovals ? { removedIds } : undefined);
      }
      previous = update.state;
    } catch {
      connected = false;
      generation++;
      dispatch("clipboard-host-unavailable");
      await new Promise(resolve => setTimeout(resolve, backoff));
      backoff = Math.min(4000, backoff * 2);
    }
  }
}

export function hostGeneration() {
  if (!connected) throw new Error("ClipboardHostUnavailable");
  return generation;
}

export async function listen<T>(event: string, receive: (message: DesktopEvent<T>) => void) {
  const id = ++nextId;
  const handler = (message: Event) => receive({ event, id, payload: (message as CustomEvent<T>).detail });
  bus.addEventListener(event, handler);
  if (event === "open-settings-panel") settingsListeners++;
  if (event === "focus-search-input") searchListeners++;
  if (!watching) {
    observeNativeAppearance();
    watching = true;
    window.mygo.on<string>("hotkey-recorded", hotkey => dispatch("hotkey-recorded", hotkey));
    window.mygo.on("recording-cancelled", () => dispatch("recording-cancelled"));
    window.mygo.on("settings-requested", () => { void receiveSettingsRequest().catch(console.error); });
    window.mygo.on("search-requested", () => { void receiveSearchRequest().catch(console.error); });
    window.mygo.on<boolean>("window-focus", focused => dispatch("mygo-window-focus", focused));
    window.mygo.on<boolean>("window-visibility", visible => dispatch(visible ? "privacy-view-shown" : "privacy-view-hidden"));
    window.mygo.on("window-moved", () => dispatch("mygo-window-moved"));
    window.mygo.on<number>("main-window-prepare-show", request => dispatch("main-window-prepare-show", request));
    window.mygo.on<boolean>("edge-pinned", pinned => dispatch("window-pinned-changed", pinned));
    window.mygo.on<string[]>("window-file-drop", paths => dispatch("tauri://drag-drop", { paths }));
    void watchHost();
  }
  if (event === "focus-search-input") void receiveSearchRequest().catch(console.error);
  return () => {
    if (event === "open-settings-panel") settingsListeners--;
    if (event === "focus-search-input") searchListeners--;
    bus.removeEventListener(event, handler);
  };
}

export async function emit<T>(event: string, payload?: T) {
  if (event === "settings-panel-listener-ready") await receiveSettingsRequest();
  dispatch(event, payload);
}

export async function emitTo<T>(target: string, event: string, payload?: T) {
  if (target !== "main") throw new Error(`Unknown MyGo WebView target: ${target}`);
  dispatch(event, payload);
}
