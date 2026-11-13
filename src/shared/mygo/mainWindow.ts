import type { Event as DesktopEvent } from "@tauri-apps/api/event";
import { shellCall } from "./bridge";
import { listen } from "./events";

// MyGo's Linux title-bar bridge does not honor DisableMaximize. Intercept only
// repeated presses in our drag region, before the document bridge sees them.
window.addEventListener("mousedown", event => {
  if (event.button !== 0 || event.detail < 2 || !(event.target instanceof Element)) return;
  if (!event.target.closest("[data-tauri-drag-region]") || event.target.closest("button,input,textarea,select,a,[contenteditable=true]")) return;
  event.preventDefault();
  event.stopPropagation();
}, true);

const systemTheme = () => window.matchMedia("(prefers-color-scheme: dark)");
export const mainWindow = {
  label: "main",
  listen,
  isVisible: () => shellCall<boolean>("IsVisible"),
  isFocused: () => shellCall<boolean>("IsFocused"),
  theme: async () => systemTheme().matches ? "dark" : "light",
  onFocusChanged: (handler: (event: DesktopEvent<boolean>) => void) => listen("mygo-window-focus", handler),
  onMoved: (handler: () => void) => listen("mygo-window-moved", handler),
  onThemeChanged: async (handler: () => void) => {
    const query = systemTheme();
    query.addEventListener("change", handler);
    return () => query.removeEventListener("change", handler);
  },
};
