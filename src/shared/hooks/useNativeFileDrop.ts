import { useEffect, useRef } from "react";
import { getCurrentWindow } from "../lib/desktopWindow";
import { invoke } from "../lib/desktopInvoke";
import { getMainPageStamp, getPrivacyViewState } from "../lib/privacySession";

export function useNativeFileDrop(onError: (error: unknown) => void) {
  const report = useRef(onError);
  report.current = onError;
  useEffect(() => {
    let disposed = false;
    const pending = new Set<string>();
    const stop = getCurrentWindow().listen<{ paths: string[] }>("tauri://drag-drop", event => {
      const paths = event.payload?.paths;
      if (disposed || !Array.isArray(paths) || !paths.length) return;
      const key = JSON.stringify(paths);
      if (pending.has(key)) return;
      const privacy = getPrivacyViewState();
      const stamp = getMainPageStamp();
      if (privacy.phase !== "ready" || !privacy.snapshot || !stamp || privacy.pending) {
        report.current(new Error("PrivacyStateUnavailable")); return;
      }
      pending.add(key);
      void invoke("copy_dropped_files", { paths, expectedHostInstanceId: stamp.instanceId,
        expectedPrivacyEpoch: privacy.snapshot.privacy_epoch }).catch(error => report.current(error))
        .finally(() => pending.delete(key));
    }).catch(error => {
      if (!disposed) report.current(error);
      return () => {};
    });
    const preventNavigation = (event: DragEvent) => {
      if (event.dataTransfer?.types.includes("Files")) {
        event.preventDefault();
        event.dataTransfer.dropEffect = "copy";
      }
    };
    window.addEventListener("dragover", preventNavigation);
    window.addEventListener("drop", preventNavigation);
    return () => {
      disposed = true;
      void stop.then(unlisten => unlisten());
      window.removeEventListener("dragover", preventNavigation);
      window.removeEventListener("drop", preventNavigation);
    };
  }, []);
}
