import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { createCapturedEntryReader } from "../lib/capturedEntryReader";
import type { ClipboardEntry } from "../types";
import { invokePrivacyBound, subscribePrivacyInvalidation } from "../lib/privacySession";
import { entryInstanceArgs } from "../lib/historyIdentity";

interface UseClipboardEventsOptions {
  onUpdated: (entry: ClipboardEntry) => void;
  onRemoved: (id: number) => void;
  onChanged: () => void;
}

export const useClipboardEvents = ({ onUpdated, onRemoved, onChanged }: UseClipboardEventsOptions) => {
  const onUpdatedRef = useRef(onUpdated);
  const onRemovedRef = useRef(onRemoved);
  const onChangedRef = useRef(onChanged);

  useEffect(() => {
    onUpdatedRef.current = onUpdated;
  }, [onUpdated]);

  useEffect(() => {
    onRemovedRef.current = onRemoved;
  }, [onRemoved]);

  useEffect(() => {
    onChangedRef.current = onChanged;
  }, [onChanged]);

  useEffect(() => {
    let disposed = false;
    const captured = createCapturedEntryReader(
      (id, hostInstanceId) => invokePrivacyBound<ClipboardEntry>("get_captured_entry_summary", { id, ...entryInstanceArgs(hostInstanceId) }),
      (entry) => onUpdatedRef.current(entry),
      () => onChangedRef.current(),
    );
    const unlistenCaptured = listen<{ id: number; host_instance_id?: string }>("clipboard-captured", (event) => {
      captured.capture(event.payload.id, event.payload.host_instance_id);
    });
    const unlistenUpdate = listen<{ id: number; host_instance_id?: string }>("clipboard-updated", (event) => {
      if (disposed) return;
      captured.capture(event.payload.id, event.payload.host_instance_id);
    });
    const unlistenRemove = listen<number>("clipboard-removed", (event) => {
      if (disposed) return;
      const interrupted = captured.invalidate();
      onRemovedRef.current(event.payload);
      if (interrupted) onChangedRef.current();
    });
    const unlistenChanged = listen("clipboard-changed", () => {
      if (disposed) return;
      captured.invalidate();
      onChangedRef.current();
    });
    const stopPrivacy = subscribePrivacyInvalidation(() => { captured.invalidate(); });
    void Promise.all([unlistenCaptured, unlistenUpdate, unlistenRemove, unlistenChanged])
      .catch(() => {
        if (!disposed) onChangedRef.current();
      });

    return () => {
      disposed = true;
      stopPrivacy();
      captured.dispose();
      for (const unlisten of [unlistenCaptured, unlistenUpdate, unlistenRemove, unlistenChanged]) {
        void unlisten.then((f) => f(), () => undefined);
      }
    };
  }, []);
};


