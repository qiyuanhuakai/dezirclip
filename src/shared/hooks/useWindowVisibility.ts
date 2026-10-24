import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

export const useWindowVisibility = () => {
  const isWindowVisibleRef = useRef(true);

  useEffect(() => {
    let active = true;
    let revision = 0;
    const readVisibility = () => {
      const request = ++revision;
      void getCurrentWindow().isVisible().then(visible => {
        if (active && request === revision) isWindowVisibleRef.current = visible;
      }, () => { if (active && request === revision) isWindowVisibleRef.current = false; });
    };
    const subscriptions = [
      getCurrentWindow().onFocusChanged(readVisibility),
      listen("privacy-view-hidden", () => { revision += 1; isWindowVisibleRef.current = false; }),
      listen("privacy-view-shown", readVisibility),
    ];
    subscriptions.forEach(subscription => {
      void subscription.catch(() => { if (active) isWindowVisibleRef.current = false; });
    });
    readVisibility();

    return () => {
      active = false;
      revision += 1;
      subscriptions.forEach(subscription => { void subscription.then(stop => stop(), () => undefined); });
    };
  }, []);

  return isWindowVisibleRef;
};
