import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";

interface UseToastListenerOptions {
  pushToast: (msg: string, duration?: number) => number;
  t: (key: string) => string;
}

/**
 * Subscribes to the backend's `toast` channel.
 *
 * The payload is an i18n key, not finished text: the backend has no locale, and
 * emitting English there would have shown English in a Chinese build. Missing
 * keys fall back to the key itself, so a typo is visible rather than silent.
 */
export const useToastListener = ({ pushToast, t }: UseToastListenerOptions) => {
  useEffect(() => {
    const unlistenToast = listen<string>("toast", (event) => {
      pushToast(t(event.payload), 3000);
    });
    return () => {
      unlistenToast.then((f) => f());
    };
  }, [pushToast, t]);
};
