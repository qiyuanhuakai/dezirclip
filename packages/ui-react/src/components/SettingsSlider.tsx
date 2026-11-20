import type { ComponentProps, CSSProperties } from "react";

export type SettingsSliderProps = Omit<ComponentProps<"input">, "type" | "min" | "max" | "value"> & {
  min: number;
  max: number;
  value: number;
};

export function SettingsSlider({ min, max, value, style, ...props }: SettingsSliderProps) {
  const progress = max > min ? Math.min(100, Math.max(0, ((value - min) / (max - min)) * 100)) : 0;
  const appearance = {
    width: "100%",
    cursor: "pointer",
    accentColor: "var(--accent-color)",
    "--range-progress": `${progress}%`,
    ...style,
  } as CSSProperties;
  return <input {...props} type="range" min={min} max={max} value={value} style={appearance} />;
}
