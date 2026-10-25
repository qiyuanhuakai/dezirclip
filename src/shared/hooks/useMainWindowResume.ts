import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { createMainWindowResume, type PreparedResume } from "../lib/mainWindowResume";
import { prepareMainWindowResume } from "../lib/privacySession";

export function useMainWindowResume(settingsLoaded: boolean, fetchHistory: (reset?: boolean) => Promise<boolean>, historyCount: number, historyViewActive: boolean) {
  const [phase, setPhase] = useState<"idle" | "preparing" | "prepared" | "committing" | "failed">("preparing");
  const [prepared, setPrepared] = useState<PreparedResume | null>(null);
  const fetchRef = useRef(fetchHistory);
  fetchRef.current = fetchHistory;
  const retryRef = useRef(() => {});
  const cancelRef = useRef(() => {});
  const nativeActiveRef = useRef(false);
  const visibleRecoveryPending = useRef(false);
  const phaseRef = useRef(phase);
  phaseRef.current = phase;
  useEffect(() => {
    if (!settingsLoaded) return;
    let disposed = false;
    const controller = createMainWindowResume({
      prepare: prepareMainWindowResume,
      readAuthority: () => invoke("get_clipboard_host_status"),
      loadHistory: () => fetchRef.current(true),
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
      void invoke<number | null>("get_main_window_resume_request").then(id => {
        if (disposed || query !== queryRevision) return;
        if (id !== null) start(id);
        else if (allowVisibleRecovery && !nativeActiveRef.current && !visibleRecoveryPending.current) {
          void getCurrentWindow().isVisible().then(visible => {
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
      listen<number>("main-window-prepare-show", event => start(event.payload)),
      listen("privacy-view-hidden", () => {
        if (disposed) return;
        queryRevision += 1;
        nativeActiveRef.current = false;
        visibleRecoveryPending.current = false;
        controller.cancel(); setPrepared(null); setPhase("preparing");
      }),
      listen("privacy-view-shown", shown),
      getCurrentWindow().onFocusChanged(event => { if (event.payload) shown(); })
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
    const root = document.querySelector(".main-resume-page");
    if (!root) return;
    const check = () => {
      if (!prepared.isRequestCurrent()) return;
      if (!prepared.isCurrent()) { cancelRef.current(); nativeActiveRef.current = false; visibleRecoveryPending.current = false; setPhase("failed"); return; }
      if (!historyViewActive || historyCount === 0 || root.querySelector(".history-item")) setPhase("committing");
    };
    const observer = new MutationObserver(check);
    observer.observe(root, { childList: true, subtree: true });
    check();
    return () => { observer.disconnect(); };
  }, [prepared, phase, historyCount, historyViewActive]);
  useLayoutEffect(() => {
    if (!prepared || phase !== "committing") return;
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
