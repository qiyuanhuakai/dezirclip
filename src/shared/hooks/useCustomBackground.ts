import { useEffect } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
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
    const apply = () => {
      root.style.setProperty("--custom-bg-opacity", (customBackgroundOpacity / 100).toString());
      if (customBackground && ["mica", "acrylic", "liquid-glass"].includes(resolveBuiltinTheme(theme))) {
        root.style.setProperty("--custom-bg-image", `url("${convertFileSrc(customBackground)}")`);
        body.classList.add("has-custom-bg");
      } else {
        root.style.removeProperty("--custom-bg-image");
        body.classList.remove("has-custom-bg");
      }
    };
    apply();
    return subscribeCustomThemes(apply);
  }, [customBackground, theme, customBackgroundOpacity]);
};
