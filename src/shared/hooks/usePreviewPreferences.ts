import { createContext, useCallback, useEffect, useRef, useState } from "react";
import { loadPreviewPreferences, subscribePreviewPreferenceChanges } from "../lib/previewPreferenceClient";
import { readPreviewPreferences } from "../lib/previewPreferences";

export const PreviewOpacityContext = createContext(readPreviewPreferences({}).opacity);
import { usePrivacySession } from "./usePrivacySession";

export function usePreviewPreferences() {
  const privacy = usePrivacySession();
  const [preferences, setPreferences] = useState(() => readPreviewPreferences({}));
  const [ready, setReady] = useState(false);
  const [error, setError] = useState(false);
  const version = useRef(0);
  const mounted = useRef(false);
  const reload = useCallback(async () => {
    const request = ++version.current;
    try {
      const next = await loadPreviewPreferences();
      if (!mounted.current || request !== version.current) return;
      setPreferences(next);
      setReady(true);
      setError(false);
    } catch {
      if (mounted.current && request === version.current) setError(true);
    }
  }, []);
  useEffect(() => {
    mounted.current = true;
    let disposed = false;
    let stop: (() => void) | undefined;
    void subscribePreviewPreferenceChanges(() => { if (!disposed) void reload(); }).then(unlisten => {
      if (disposed) { unlisten(); return; }
      stop = unlisten;
      void reload();
    }, () => { if (!disposed) setError(true); });
    return () => { disposed = true; mounted.current = false; version.current += 1; stop?.(); };
  }, [reload]);
  const canSave = import.meta.env.MODE !== "mygo" || (privacy.phase === "ready" && !!privacy.snapshot && !privacy.pending);
  return { preferences, ready: ready && canSave, error, reload };
}
