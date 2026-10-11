import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

/**
 * Tell the backend the main window has drawn a frame worth showing.
 *
 * The idle destroyer and the GPU switch tear the webview down and rebuild it
 * from nothing, and the backend holds the replacement hidden until this arrives.
 * Without it, showing straight after `build()` puts an empty window on screen for
 * the length of a boot — and with a short idle timeout that is the path *every*
 * hotkey press takes, which is why the flash was constant rather than rare.
 *
 * Waits for settings first, because "painted" has to mean "there is something to
 * show" rather than "an empty shell reached the screen", then for two animation
 * frames so the paint is on the compositor before the flag goes up.
 *
 * The backend shows the window on its own deadline regardless, so a frontend
 * that never reaches this costs a little latency and never a stuck window.
 */
export const useMainWindowPaintSignal = (settingsLoaded: boolean) => {
  useEffect(() => {
    if (!settingsLoaded) return;

    let cancelled = false;
    let inner = 0;
    const outer = requestAnimationFrame(() => {
      inner = requestAnimationFrame(() => {
        if (cancelled) return;
        void invoke("notify_main_window_painted").catch(() => undefined);
      });
    });

    return () => {
      cancelled = true;
      cancelAnimationFrame(outer);
      cancelAnimationFrame(inner);
    };
  }, [settingsLoaded]);
};
