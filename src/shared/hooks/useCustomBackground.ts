import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { resolveBuiltinTheme } from "../lib/themeRuntime";
import { subscribeCustomThemes } from "../lib/themeCatalog";

interface UseCustomBackgroundOptions {
  customBackground: string;
  customBackgroundOpacity: number;
  theme: string;
}

export const useCustomBackground = ({
  customBackground,
  customBackgroundOpacity,
  theme
}: UseCustomBackgroundOptions) => {
  useEffect(() => {
    const root = document.documentElement;
    const body = document.body;
    let generation = 0;
    let disposed = false;
    let stopBackgroundChanges: UnlistenFn | undefined;
    const apply = () => {
      const current = ++generation;
      root.style.setProperty("--custom-bg-opacity", (customBackgroundOpacity / 100).toString());
      root.style.removeProperty("--custom-bg-image");
      body.classList.remove("has-custom-bg");
      if (customBackground && ["mica", "acrylic", "liquid-glass"].includes(resolveBuiltinTheme(theme))) {
        void invoke<string>("get_custom_background_resource", { expectedPath: customBackground }).then(src => {
          if (disposed || current !== generation || !/^data:image\/(png|jpeg|gif|webp);base64,[A-Za-z0-9+/=]+$/.test(src)) return;
          root.style.setProperty("--custom-bg-image", `url("${src}")`);
          body.classList.add("has-custom-bg");
        }).catch(() => undefined);
      }
    };
    const stop = subscribeCustomThemes(apply);
    // Selection updates React before save_setting has committed the path. Read
    // after subscribing so the save acknowledgement cannot fall between the
    // first authorized read and installing its listener.
    void listen<string>("custom-background-changed", () => apply()).then(unlisten => {
      if (disposed) { unlisten(); return; }
      stopBackgroundChanges = unlisten;
      apply();
    }).catch(() => { if (!disposed) apply(); });
    return () => {
      disposed = true;
      generation += 1;
      stop();
      stopBackgroundChanges?.();
    };
  }, [customBackground, theme, customBackgroundOpacity]);
};
