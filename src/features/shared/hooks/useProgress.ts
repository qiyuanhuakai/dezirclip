import { useEffect, useState, useCallback, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

export type ProgressToastItem = {
  id: string;
  message: string;
  progress: number;
  status: "running" | "done" | "error";
};

const MAX_VISIBLE = 3;
const DISMISS_DELAY_MS = 3000;

export const useProgress = () => {
  const [toasts, setToasts] = useState<ProgressToastItem[]>([]);
  const dismissTimers = useRef<Map<string, ReturnType<typeof setTimeout>>>(
    new Map()
  );

  useEffect(() => {
    const unlisten = listen<ProgressToastItem>(
      "dezirclip://progress",
      (event) => {
        const payload = event.payload;
        if (!payload || !payload.id) return;

        const existingTimer = dismissTimers.current.get(payload.id);
        if (existingTimer) {
          clearTimeout(existingTimer);
          dismissTimers.current.delete(payload.id);
        }

        setToasts((prev) => {
          const idx = prev.findIndex((t) => t.id === payload.id);
          if (idx >= 0) {
            const next = [...prev];
            next[idx] = payload;
            return next;
          }
          return [payload, ...prev].slice(0, MAX_VISIBLE);
        });

        if (payload.status === "done") {
          const timer = setTimeout(() => {
            setToasts((prev) => prev.filter((t) => t.id !== payload.id));
            dismissTimers.current.delete(payload.id);
          }, DISMISS_DELAY_MS);
          dismissTimers.current.set(payload.id, timer);
        }
      }
    );

    return () => {
      unlisten.then((f) => f());
      dismissTimers.current.forEach((t) => clearTimeout(t));
      dismissTimers.current.clear();
    };
  }, []);

  const dismiss = useCallback((id: string) => {
    const timer = dismissTimers.current.get(id);
    if (timer) {
      clearTimeout(timer);
      dismissTimers.current.delete(id);
    }
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  return { toasts, dismiss };
};
