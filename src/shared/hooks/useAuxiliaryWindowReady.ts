import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";

export const useAuxiliaryWindowReady = (enabled: boolean) => {
  const reported = useRef(false);

  useEffect(() => {
    if (!enabled || reported.current) return;

    const generation = Number(new URLSearchParams(window.location.search).get("instance"));
    if (!Number.isSafeInteger(generation) || generation <= 0) return;

    let cancelled = false;
    let retryTimer: number | undefined;
    let retryDelay = 100;

    // Effects run after the DOM commit. Hidden WebKit pauses animation frames,
    // so this initialization acknowledgement must not depend on a paint.
    const reportReady = async () => {
      try {
        const accepted = await invoke<boolean>("notify_auxiliary_window_ready", { generation });
        if (cancelled) return;
        if (accepted) {
          reported.current = true;
          return;
        }
      } catch {
        if (cancelled) return;
      }

      retryTimer = window.setTimeout(() => {
        retryTimer = undefined;
        void reportReady();
      }, retryDelay);
      retryDelay = Math.min(retryDelay * 2, 1000);
    };
    void reportReady();

    return () => {
      cancelled = true;
      if (retryTimer !== undefined) window.clearTimeout(retryTimer);
    };
  }, [enabled]);
};
