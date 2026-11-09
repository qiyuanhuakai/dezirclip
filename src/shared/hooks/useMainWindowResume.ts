import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "../lib/desktopInvoke";
import { getCurrentWindow } from "../lib/desktopWindow";
import { createMainWindowResume, type PreparedResume } from "../lib/mainWindowResume";
import { prepareMainWindowResume } from "../lib/privacySession";
import type { PageStamp } from "../lib/resumePage";

export function useMainWindowResume(settingsLoaded: boolean, fetchHistory: (reset?: boolean, stamp?: PageStamp) => Promise<boolean>, historyViewActive: boolean) {
  const [phase, setPhase] = useState<"idle" | "preparing" | "prepared" | "failed">("preparing");
  const [prepared, setPrepared] = useState<PreparedResume | null>(null);
  const fetchRef = useRef(fetchHistory);
  fetchRef.current = fetchHistory;
  const historyView = useRef(historyViewActive);
  historyView.current = historyViewActive;
  const retryRef = useRef(() => {});
  const cancelRef = useRef(() => {});
  const nativeActiveRef = useRef(false);
  const visibleRecoveryPending = useRef(false);
  const phaseRef = useRef(phase);
  phaseRef.current = phase;
  useEffect(() => {
    if (!settingsLoaded) return;
    const currentWindow = getCurrentWindow();
    let disposed = false;
    const controller = createMainWindowResume({
      prepare: prepareMainWindowResume,
      readAuthority: () => invoke("get_clipboard_host_status"),
      loadHistory: stamp => historyView.current ? fetchRef.current(true, stamp) : Promise.resolve(true),
      pending: requestId => { nativeActiveRef.current = requestId !== null; setPrepared(null); setPhase("preparing"); },
      prepared: value => { setPrepared(value); setPhase("prepared"); },
      failed: () => { nativeActiveRef.current = false; visibleRecoveryPending.current = false; setPrepared(null); setPhase("failed"); }
    });
    cancelRef.current = controller.cancel;
    let received = false;
    let queryRevision = 0;
    let subscriptionsFailed = false;
    const start = (id: number) => {
      if (disposed || subscriptionsFailed) return;
      queryRevision += 1;
      visibleRecoveryPending.current = false;
      received = true;
      void controller.start(id);
    };
    const queryPending = (allowVisibleRecovery = false) => {
      const query = ++queryRevision;
      const pending = invoke<number | null>("get_main_window_resume_request");
      void pending.then(id => {
        if (disposed || query !== queryRevision) return;
        if (id !== null) start(id);
        else if (allowVisibleRecovery && !nativeActiveRef.current && !visibleRecoveryPending.current) {
          void currentWindow.isVisible().then(visible => {
            if (!visible || disposed || query !== queryRevision || nativeActiveRef.current || visibleRecoveryPending.current) return;
            received = true;
            queryRevision += 1;
            visibleRecoveryPending.current = true;
            void controller.restoreVisible();
          }, () => { if (!disposed && query === queryRevision) setPhase("failed"); });
        }
        else if (!received) setPhase("idle");
      }, () => { if (!disposed && query === queryRevision) setPhase("failed"); });
    };
    retryRef.current = () => queryPending(true);
    const shown = () => {
      if (!disposed && !nativeActiveRef.current && !visibleRecoveryPending.current && phaseRef.current !== "idle") queryPending(true);
    };
    const subscriptions = [
      currentWindow.listen<number>("main-window-prepare-show", event => start(event.payload)),
      currentWindow.listen("privacy-view-hidden", () => {
        if (disposed) return;
        queryRevision += 1;
        nativeActiveRef.current = false;
        visibleRecoveryPending.current = false;
        controller.cancel(); setPrepared(null); setPhase("preparing");
      }),
      currentWindow.listen("privacy-view-shown", shown),
      currentWindow.listen("clipboard-host-changed", () => {
        if (disposed) return;
        controller.cancel();
        nativeActiveRef.current = false;
        visibleRecoveryPending.current = false;
        setPrepared(null); setPhase("preparing");
        queryPending(true);
      }),
      currentWindow.onFocusChanged(event => { if (event.payload) shown(); })
    ];
    void Promise.all(subscriptions).then(() => queryPending(), () => {
      if (disposed) return;
      subscriptionsFailed = true;
      controller.cancel(); setPrepared(null); setPhase("failed");
    });
    return () => {
      disposed = true;
      controller.cancel();
      retryRef.current = () => {};
      cancelRef.current = () => {};
      subscriptions.forEach(subscription => { void subscription.then(stop => stop(), () => undefined); });
    };
  }, [settingsLoaded]);
  useLayoutEffect(() => {
    if (!prepared || phase !== "prepared") return;
    // The masked page has committed. A hidden virtualizer may not measure its first row until show.
    let disposed = false;
    const failed = () => { cancelRef.current(); nativeActiveRef.current = false; visibleRecoveryPending.current = false; setPhase("failed"); };
    if (!prepared.isRequestCurrent()) return;
    if (!prepared.isCurrent()) { failed(); return; }
    if (prepared.requestId === null) {
      visibleRecoveryPending.current = false;
      setPhase("idle");
      return;
    }
    // History state and this prepared marker commit in the same React tree.
    void invoke<boolean>("complete_main_window_resume", {
      requestId: prepared.requestId,
      expectedHostInstanceId: prepared.authority.instanceId,
      expectedEpoch: prepared.authority.epoch
    }).then(accepted => {
      if (disposed || !prepared.isRequestCurrent()) return;
      if (accepted && prepared.isCurrent()) { nativeActiveRef.current = false; setPhase("idle"); } else failed();
    }, () => { if (!disposed && prepared.isRequestCurrent()) failed(); });
    return () => { disposed = true; };
  }, [prepared, phase]);
  return { phase, mode: prepared?.requestId === null ? "already_visible" : "before_show", retry: () => retryRef.current() };
}
