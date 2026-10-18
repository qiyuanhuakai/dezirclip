import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";

export const useAuxiliaryWindowReady = (enabled: boolean) => {
  const reported = useRef(false);

  useEffect(() => {
    if (!enabled || reported.current) return;

    const generation = Number(new URLSearchParams(window.location.search).get("instance"));
    if (!Number.isSafeInteger(generation) || generation <= 0) return;

    let cancelled = false;
    let inner = 0;
    const outer = requestAnimationFrame(() => {
      inner = requestAnimationFrame(() => {
        if (cancelled) return;
        reported.current = true;
        void invoke("notify_auxiliary_window_ready", { generation }).catch(() => undefined);
      });
    });

    return () => {
      cancelled = true;
      cancelAnimationFrame(outer);
      cancelAnimationFrame(inner);
    };
  }, [enabled]);
};
