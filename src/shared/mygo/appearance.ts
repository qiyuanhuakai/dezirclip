import { nativeSurfaceAppearance } from "@deziro/ui-react/native-appearance";
export { nativeSurfaceAppearance };
import { createLatestAppearanceSender } from "./latestAppearanceSender";

export function observeNativeAppearance() {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let previous = "";
  const sender = createLatestAppearanceSender(async (appearance: ReturnType<typeof nativeSurfaceAppearance>) => {
    const key = JSON.stringify(appearance);
    if (key === previous) return;
    await window.mygo.call("DesktopShell.SyncAppearance", appearance);
    previous = key;
  }, console.error);
  const schedule = () => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      const appearance = nativeSurfaceAppearance();
      sender.offer(appearance);
    }, 150);
  };
  const observer = new MutationObserver(schedule);
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "style", "lang"] });
  observer.observe(document.head, { childList: true, subtree: true });
  document.addEventListener("load", schedule, true);
  window.addEventListener("dezirclip-theme-applied", schedule);
  schedule();
  window.addEventListener("pagehide", () => { sender.close(); observer.disconnect(); clearTimeout(timer); document.removeEventListener("load", schedule, true); window.removeEventListener("dezirclip-theme-applied", schedule); }, { once: true });
}
