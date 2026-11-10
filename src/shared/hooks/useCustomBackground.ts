import { useEffect, useRef, useState } from "react";
import { invoke } from "../lib/desktopInvoke";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { resolveBuiltinTheme } from "../lib/themeRuntime";
import { subscribeCustomThemes } from "../lib/themeCatalog";
import { decodeBackgroundImage } from "../lib/backgroundImage";

interface UseCustomBackgroundOptions {
  customBackground: string;
  customBackgroundOpacity: number;
  theme: string;
  privacyGeneration?: number | null;
  onError: () => void;
}

export const useCustomBackground = ({
  customBackground,
  customBackgroundOpacity,
  theme,
  privacyGeneration,
  onError
}: UseCustomBackgroundOptions) => {
  const reportError = useRef(onError);
  const appliedUrl = useRef<string | null>(null);
  const [settled, setSettled] = useState<{ background: string; theme: string; privacyGeneration: number | null | undefined } | null>(null);
  reportError.current = onError;
  useEffect(() => () => {
    document.documentElement.style.removeProperty("--custom-bg-image");
    document.body.classList.remove("has-custom-bg", "opaque-background-image");
    if (appliedUrl.current) URL.revokeObjectURL(appliedUrl.current);
    appliedUrl.current = null;
  }, []);
  useEffect(() => {
    const opacity = Number.isFinite(customBackgroundOpacity) ? Math.min(100, Math.max(0, customBackgroundOpacity)) / 100 : 0.45;
    document.documentElement.style.setProperty("--custom-bg-opacity", String(opacity));
    document.body.classList.toggle("opaque-background-image", opacity === 1);
  }, [customBackgroundOpacity]);
  useEffect(() => {
    const root = document.documentElement;
    const body = document.body;
    let generation = 0;
    let disposed = false;
    let stopBackgroundChanges: UnlistenFn | undefined;
    let deadline: ReturnType<typeof setTimeout> | undefined;
    const complete = () => {
      clearTimeout(deadline);
      setSettled({ background: customBackground, theme, privacyGeneration });
    };
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
    // MyGo reads the selected image through the authenticated desktop session.
    if (privacyGeneration === null) {
      clearAppliedImage();
      setSettled(null);
      return;
    }
    const apply = () => {
      const current = ++generation;
      clearTimeout(deadline);
      setSettled(null);
      cancelPendingImages();
      if (customBackground && ["mica", "acrylic", "liquid-glass"].includes(resolveBuiltinTheme(theme))) {
        // A missing/unreadable resource must not hold a hidden window forever.
        // Retire this read on timeout so it cannot pop in after the fallback shows.
        deadline = setTimeout(() => {
          if (disposed || current !== generation) return;
          generation += 1;
          cancelPendingImages();
          clearAppliedImage();
          complete();
          reportError.current();
        }, 5000);
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
          complete();
        }).catch((error: unknown) => {
          if (disposed || current !== generation || error === "CustomBackgroundPending") return;
          cancelPendingImages();
          clearAppliedImage();
          complete();
          console.warn("Custom background could not be loaded");
          reportError.current();
        });
      } else { clearAppliedImage(); complete(); }
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
      clearTimeout(deadline);
      cancelPendingImages();
      stop();
      stopBackgroundChanges?.();
    };
  }, [customBackground, theme, privacyGeneration]);
  return privacyGeneration !== null && settled !== null && settled.background === customBackground
    && settled.theme === theme && settled.privacyGeneration === privacyGeneration;
};
