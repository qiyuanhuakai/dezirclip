import { useEffect } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

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
    root.style.setProperty("--custom-bg-opacity", (customBackgroundOpacity / 100).toString());
    if (customBackground && (theme === "mica" || theme === "acrylic" || theme === "liquid-glass")) {
      root.style.setProperty("--custom-bg-image", `url("${convertFileSrc(customBackground)}")`);
      body.classList.add("has-custom-bg");
    } else {
      root.style.removeProperty("--custom-bg-image");
      body.classList.remove("has-custom-bg");
    }
  }, [customBackground, theme, customBackgroundOpacity]);
};
