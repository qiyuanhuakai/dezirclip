import type { Event as DesktopEvent } from "@tauri-apps/api/event";

const bus = new EventTarget();
let nextId = 0;
let watching = false;
let connected = false;
let generation = 0;

interface HostState {
  service_instance_id: string;
  privacy_epoch: number;
  history_revision: number;
  event_sequence: number;
}
interface Changes {
  state: HostState;
  events: { event: string; data: { entry_id?: number; status?: string }; state: HostState }[];
  resync_required: boolean;
}

function dispatch(name: string, payload?: unknown) {
  bus.dispatchEvent(new CustomEvent(name, { detail: payload }));
}

async function watchHost() {
  let previous: HostState | undefined;
  let backoff = 250;
  for (;;) {
    try {
      const update = await window.mygo.call<Changes>("History.Changes", previous?.event_sequence ?? null);
      const recovered = !connected;
      connected = true;
      backoff = 250;
      const resync = recovered || update.resync_required || previous?.service_instance_id !== update.state.service_instance_id;
      const privacyChanged = previous?.privacy_epoch !== update.state.privacy_epoch;
      if (resync || privacyChanged) generation++;
      if (resync) dispatch("clipboard-host-changed", update.state.service_instance_id);
      if (privacyChanged) dispatch("privacy-state-changed");
      let settingsChanged = resync || privacyChanged;
      for (const { event, data, state } of update.events) {
        if (state.privacy_epoch !== update.state.privacy_epoch || state.service_instance_id !== update.state.service_instance_id) continue;
        if ((event === "EntryAdded" || event === "EntryUpdated") && data.entry_id !== undefined) {
          dispatch("clipboard-captured", { id: data.entry_id, host_instance_id: state.service_instance_id });
          if (event === "EntryUpdated" && data.status && ["processing", "done", "failed", "unsupported"].includes(data.status)) {
            dispatch("ocr:complete", { item_id: data.entry_id, text: "", confidence: null, status: data.status });
          }
        } else if (event === "EntryDeleted") {
          dispatch("clipboard-removed", data.entry_id);
        } else if (event === "HistoryReset") {
          dispatch("tag-colors-updated");
        } else if (event === "ServiceStateChanged" && data.status && ["capture_settings_changed", "view_settings_changed", "custom_themes_changed"].includes(data.status)) {
          settingsChanged = true;
        } else if (event === "ServiceStateChanged" && data.status?.startsWith("paste_queue_changed:")) {
          const count = Number(data.status.slice("paste_queue_changed:".length));
          if (Number.isSafeInteger(count) && count >= 0) dispatch("paste-queue-changed", count);
        }
      }
      if (settingsChanged) dispatch("settings-changed");
      if (recovered || update.resync_required || previous?.history_revision !== update.state.history_revision) dispatch("clipboard-changed");
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
  if (!watching) {
    watching = true;
    window.mygo.on<boolean>("window-focus", focused => dispatch("mygo-window-focus", focused));
    window.mygo.on<boolean>("window-visibility", visible => dispatch(visible ? "privacy-view-shown" : "privacy-view-hidden"));
    window.mygo.on("window-moved", () => dispatch("mygo-window-moved"));
    window.mygo.on<string[]>("window-file-drop", paths => dispatch("tauri://drag-drop", { paths }));
    void watchHost();
  }
  return () => bus.removeEventListener(event, handler);
}

export async function emit<T>(event: string, payload?: T) { dispatch(event, payload); }

export async function emitTo<T>(target: string, event: string, payload?: T) {
  if (target !== "main") throw new Error(`Unknown MyGo WebView target: ${target}`);
  dispatch(event, payload);
}
