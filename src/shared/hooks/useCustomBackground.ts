import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { resolveBuiltinTheme } from "../lib/themeRuntime";
import { subscribeCustomThemes } from "../lib/themeCatalog";
import { decodeBackgroundImage } from "../lib/backgroundImage";

interface UseCustomBackgroundOptions {
  customBackground: string;
  customBackgroundOpacity: number;
  theme: string;
  onError: () => void;
}

export const useCustomBackground = ({
  customBackground,
  customBackgroundOpacity,
  theme,
  onError
}: UseCustomBackgroundOptions) => {
  const reportError = useRef(onError);
  const appliedUrl = useRef<string | null>(null);
  reportError.current = onError;
  useEffect(() => () => {
    document.documentElement.style.removeProperty("--custom-bg-image");
    document.body.classList.remove("has-custom-bg");
    if (appliedUrl.current) URL.revokeObjectURL(appliedUrl.current);
    appliedUrl.current = null;
  }, []);
  useEffect(() => {
    const opacity = Number.isFinite(customBackgroundOpacity) ? Math.min(100, Math.max(0, customBackgroundOpacity)) / 100 : 0.45;
    document.documentElement.style.setProperty("--custom-bg-opacity", String(opacity));
  }, [customBackgroundOpacity]);
  useEffect(() => {
    const root = document.documentElement;
    const body = document.body;
    let generation = 0;
    let disposed = false;
    let stopBackgroundChanges: UnlistenFn | undefined;
    const pendingImages = new Map<string, HTMLImageElement>();
    const cancelPendingImages = () => {
      for (const [url, image] of pendingImages) { image.src = ""; URL.revokeObjectURL(url); }
      pendingImages.clear();
    };
    const clearAppliedImage = () => {
      root.style.removeProperty("--custom-bg-image");
      body.classList.remove("has-custom-bg");
      if (appliedUrl.current) URL.revokeObjectURL(appliedUrl.current);
      appliedUrl.current = null;
    };
    const apply = () => {
      const current = ++generation;
      cancelPendingImages();
      if (customBackground && ["mica", "acrylic", "liquid-glass"].includes(resolveBuiltinTheme(theme))) {
        void invoke<string>("get_custom_background_resource", { expectedPath: customBackground }).then(async src => {
          if (disposed || current !== generation) return;
          const { mime, bytes } = decodeBackgroundImage(src);
          const url = URL.createObjectURL(new Blob([bytes], { type: mime }));
          const image = new Image();
          pendingImages.set(url, image);
          image.src = url;
          try { await image.decode(); }
          catch (error) { pendingImages.delete(url); URL.revokeObjectURL(url); throw error; }
          if (disposed || current !== generation) { pendingImages.delete(url); URL.revokeObjectURL(url); return; }
          const previous = appliedUrl.current;
          root.style.setProperty("--custom-bg-image", `url("${url}")`);
          body.classList.add("has-custom-bg");
          appliedUrl.current = url;
          pendingImages.delete(url);
          if (previous) URL.revokeObjectURL(previous);
        }).catch((error: unknown) => {
          if (disposed || current !== generation || error === "CustomBackgroundPending") return;
          cancelPendingImages();
          clearAppliedImage();
          console.warn("Custom background could not be loaded");
          reportError.current();
        });
      } else clearAppliedImage();
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
      cancelPendingImages();
      stop();
      stopBackgroundChanges?.();
    };
  }, [customBackground, theme]);
};
