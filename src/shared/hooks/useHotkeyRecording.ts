import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "../lib/desktopInvoke";
import { completeRecording } from "../lib/recordingCompletion";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

interface RecordingOwner {
  readonly completion: AbortController;
  readonly cancel: () => void;
  readonly fail: (error: unknown) => void;
  readonly activated: (ready: boolean) => void;
  ready: boolean;
}

let activeOwner: RecordingOwner | null = null;
let modeQueue = Promise.resolve();
let nativeMode: boolean | undefined;

async function applyNativeMode(enabled: boolean) {
  if (nativeMode === enabled) return;
  try {
    await invoke<void>("set_recording_mode", { enabled });
    nativeMode = enabled;
  } catch (error) {
    nativeMode = undefined;
    throw error;
  }
}

function synchronizeNativeMode() {
  modeQueue = modeQueue.catch(() => undefined).then(async () => {
    const owner = activeOwner;
    const enabled = owner?.ready === true;
    try {
      await applyNativeMode(enabled);
      if (activeOwner === owner) owner?.activated(enabled);
    }
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
  const [nativeReady, setNativeReady] = useState(false);
  const callbacks = useRef({ onRecorded, onCancelled, onError });
  callbacks.current = { onRecorded, onCancelled, onError };
  const wasRecording = useRef(false);
  const consumed = useRef(false);
  const owner = useRef<RecordingOwner | null>(null);
  if (recording && !wasRecording.current) consumed.current = false;
  wasRecording.current = recording;

  const cancel = useCallback(() => {
    owner.current?.completion.abort();
    consumed.current = true;
    callbacks.current.onCancelled();
  }, []);
  const record = useCallback((hotkey: string) => {
    if (!wasRecording.current || consumed.current || !hotkey) return;
    const current = owner.current;
    if (!current || activeOwner !== current || current.completion.signal.aborted) return;
    consumed.current = true;
    const handler = callbacks.current;
    // Release temporary recording grabs before the shortcut owner registers the chosen chord.
    modeQueue = modeQueue.catch(() => undefined).then(() => applyNativeMode(false));
    void completeRecording(modeQueue, current.completion.signal,
      () => wasRecording.current && owner.current === current && activeOwner === current, () => {
      handler.onCancelled();
      handler.onRecorded(hotkey);
    }, error => {
      handler.onCancelled();
      handler.onError?.(error);
    });
  }, []);

  useEffect(() => {
    if (!recording) return;
    let disposed = false;
    const cleanups: UnlistenFn[] = [];
    const current: RecordingOwner = {
      completion: new AbortController(),
      cancel, ready: false,
      activated: setNativeReady,
      fail: error => { callbacks.current.onError?.(error); cancel(); },
    };
    const previous = activeOwner;
    setNativeReady(false);
    activeOwner = current;
    owner.current = current;
    previous?.cancel();
    synchronizeNativeMode();

    void (async () => {
      const results = await Promise.allSettled([
        listen<string>("hotkey-recorded", event => { if (activeOwner === current) record(event.payload); }),
        listen("recording-cancelled", () => { if (activeOwner === current) { nativeMode = false; cancel(); } }),
      ]);
      for (const result of results) if (result.status === "fulfilled") cleanups.push(result.value);
      if (disposed || activeOwner !== current) { cleanups.splice(0).forEach(stop => stop()); return; }
      const failed = results.find(result => result.status === "rejected");
      if (failed?.status === "rejected") { current.fail(failed.reason); return; }
      current.ready = true;
      synchronizeNativeMode();
    })();

    return () => {
      current.completion.abort();
      disposed = true;
      cleanups.splice(0).forEach(stop => stop());
      if (owner.current === current) owner.current = null;
      if (activeOwner === current) { activeOwner = null; synchronizeNativeMode(); }
    };
  }, [cancel, record, recording]);

  return { record, cancel, nativeReady: recording && nativeReady };
}
