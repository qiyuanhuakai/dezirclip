import { useState } from "react";
import { SettingsBackground, SettingsThemeManager, SettingsSlider } from "../src";

export function ThemeManagerFixture({ state }: { state: string }) {
  const narrow = state === "narrow";
  const [value, setValue] = useState(state === "imported" ? "custom:studio" : "macos");
  const builtins = [
    { value: "retro", label: "Retro" },
    { value: "mica", label: "Mica" },
    { value: "acrylic", label: "Acrylic" },
    { value: "liquid-glass", label: "Liquid Glass" },
    { value: "macos", label: "FlatWhite" },
    { value: "scifi", label: "Sci-Fi" },
  ];
  const imported = [{ value: "custom:studio", label: narrow ? "Studio glass with higher contrast and softer colors" : "Studio theme" }];
  return <div style={{ width: narrow ? 220 : state === "wide" ? 500 : 330 }}>
    <SettingsThemeManager label="Visual theme" builtinLabel="Built-in themes" importedLabel="Imported themes"
      value={value} builtins={builtins} imported={imported}
      importLabel="Import theme" exportLabel="Export theme" removeLabel="Remove theme"
      onChange={setValue} onImport={() => undefined} onExport={() => undefined} onRemove={() => undefined} />
  </div>;
}

export function BackgroundSettingsFixture({ state }: { state: string }) {
  const [filename, setFilename] = useState(state === "saved" ? "Workspace wallpaper.png" : "");
  const [opacity, setOpacity] = useState(65);
  return <div className="settings-view showcase-background-config" style={{ width: 330 }}>
    <SettingsBackground label="Custom background" chooseLabel={filename ? "Change background" : "Choose background"}
      filename={filename} clearLabel="Clear background" onChoose={() => setFilename("Workspace wallpaper.png")}
      onClear={() => setFilename("")}>
      {filename && <div className="setting-item column no-border" style={{ marginTop: 12 }}>
        <div className="settings-range-header">
          <span className="item-label">Background opacity</span>
          <span className="settings-range-value">{opacity}%</span>
        </div>
        <SettingsSlider aria-label="Background opacity" min={0} max={100} value={opacity} onChange={event => setOpacity(event.currentTarget.valueAsNumber)} />
      </div>}
    </SettingsBackground>
  </div>;
}
