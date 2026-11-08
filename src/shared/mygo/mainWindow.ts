import type { Event as DesktopEvent } from "@tauri-apps/api/event";
import { shellCall } from "./bridge";
import { listen } from "./events";

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
