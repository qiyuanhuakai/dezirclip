import { useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

interface RecordingOwner {
  readonly cancel: () => void;
  readonly fail: (error: unknown) => void;
  ready: boolean;
}

let activeOwner: RecordingOwner | null = null;
let modeQueue = Promise.resolve();

function synchronizeNativeMode() {
  modeQueue = modeQueue.catch(() => undefined).then(async () => {
    try { await invoke("set_recording_mode", { enabled: activeOwner?.ready === true }); }
    catch (error) {
      if (activeOwner) activeOwner.fail(error);
      else console.error("Failed to stop hotkey recording", error);
    }
  });
}

export function useHotkeyRecording({ recording, onRecorded, onCancelled, onError }: {
  readonly recording: boolean;
  readonly onRecorded: (hotkey: string) => void;
  readonly onCancelled: () => void;
  readonly onError?: (error: unknown) => void;
}) {
  const callbacks = useRef({ onRecorded, onCancelled, onError });
  callbacks.current = { onRecorded, onCancelled, onError };
  const wasRecording = useRef(false);
  const consumed = useRef(false);
  const owner = useRef<RecordingOwner | null>(null);
  if (recording && !wasRecording.current) consumed.current = false;
  wasRecording.current = recording;

  const cancel = useCallback(() => {
    consumed.current = true;
    callbacks.current.onCancelled();
  }, []);
  const record = useCallback((hotkey: string) => {
    if (!wasRecording.current || consumed.current || !hotkey) return;
    if (owner.current && activeOwner !== owner.current) return;
    consumed.current = true;
    const handler = callbacks.current;
    handler.onCancelled();
    handler.onRecorded(hotkey);
  }, []);

  useEffect(() => {
    if (!recording) return;
    let disposed = false;
    const cleanups: UnlistenFn[] = [];
    const current: RecordingOwner = {
      cancel, ready: false,
      fail: error => { callbacks.current.onError?.(error); cancel(); },
    };
    const previous = activeOwner;
    activeOwner = current;
    owner.current = current;
    previous?.cancel();
    synchronizeNativeMode();

    void (async () => {
      const results = await Promise.allSettled([
        listen<string>("hotkey-recorded", event => { if (activeOwner === current) record(event.payload); }),
        listen("recording-cancelled", () => { if (activeOwner === current) cancel(); }),
      ]);
      for (const result of results) if (result.status === "fulfilled") cleanups.push(result.value);
      if (disposed || activeOwner !== current) { cleanups.splice(0).forEach(stop => stop()); return; }
      const failed = results.find(result => result.status === "rejected");
      if (failed?.status === "rejected") { current.fail(failed.reason); return; }
      current.ready = true;
      synchronizeNativeMode();
    })();

    return () => {
      disposed = true;
      cleanups.splice(0).forEach(stop => stop());
      if (owner.current === current) owner.current = null;
      if (activeOwner === current) { activeOwner = null; synchronizeNativeMode(); }
    };
  }, [cancel, record, recording]);

  return { record, cancel };
}
