import { useState } from "react";
import { ContextMenuItem, ContextMenuSurface, EmojiAddTile, EmojiFavoriteTile, HotkeyBinding, HotkeyList, NumberField,
  SettingsChoices, SettingsDataAction, SettingsDataPath, SettingsRemoveButton, SettingsSectionCard, TagToolbar, TextArea, TextField } from "../src";
import { Ellipsis, Eye, ListChecks, ListCollapse, Plus, Tag } from "lucide-react";

export function TextFieldsFixture() {
  return <div className="showcase-fields">
    <label>Entry name<TextField defaultValue="Release checklist" /></label>
    <label>Search terms<TextField placeholder="Filter by name" /></label>
    <label>Idle timeout<NumberField defaultValue="30" /></label>
    <label>Notes<TextArea rows={3} defaultValue={"First line\nSecond line"} /></label>
  </div>;
}

const menuItems = ["Copy", "Text processing", "Privacy", "Delete"];
const items: Record<string, string[]> = {
  "Text processing": ["Uppercase", "Lowercase", "Trim spaces"],
  "Privacy": ["Ignore this source", "Protect this source", "Manage rules"],
};

export function ContextMenuFixture({ state }: { state: string }) {
  const [active, setActive] = useState("");
  const [submenuTop, setSubmenuTop] = useState(0);
  return <div className="showcase-fields">
    <div data-menu-anchor>Clipboard entry</div>
    <ContextMenuSurface style={{ left: 72, top: 43, width: 180 }}>
      {menuItems.map(name => <ContextMenuItem key={name} active={name === active} hasSubmenu={Boolean(items[name])}
        onMouseEnter={event => {
          setSubmenuTop(event.currentTarget.getBoundingClientRect().top);
          setActive(name);
        }} onClick={() => setActive(name)}>
        <span className="item-context-menu__item-label">{name}</span>
      </ContextMenuItem>)}
    </ContextMenuSurface>
    {items[active] && state !== "default" && <ContextMenuSurface submenu style={{
      left: 256, top: submenuTop, width: 168,
    }}>
      {items[active].map((name, index) => <ContextMenuItem submenu key={name} active={index === 0}>{name}</ContextMenuItem>)}
    </ContextMenuSurface>}
  </div>;
}

export function SettingsSectionFixture({ open }: { open: boolean }) {
  const [collapsed, setCollapsed] = useState(!open);
  return <SettingsSectionCard title="General settings" collapsed={collapsed} onToggle={() => setCollapsed(value => !value)}>
    <p className="item-label">Keep clipboard history</p>
  </SettingsSectionCard>;
}

export function SettingsChoicesFixture() {
  const [mode, setMode] = useState("light");
  const [channel, setChannel] = useState("preview");
  return <div className="showcase-fields">
    <span className="item-label">Color mode</span>
    <SettingsChoices label="Color mode" value={mode} onChange={setMode}
      options={[{ value: "system", label: "System" }, { value: "light", label: "Light" }, { value: "dark", label: "Dark" }]} />
    <span className="item-label">Update channel</span>
    <SettingsChoices label="Update channel" value={channel} onChange={setChannel} segmented
      options={[{ value: "stable", label: "Stable" }, { value: "preview", label: "Preview" }]} />
  </div>;
}

export function HotkeyListFixture({ multiple }: { multiple: boolean }) {
  const [values, setValues] = useState(multiple ? ["Super+V", "Ctrl+Shift+V"] : ["Super+V"]);
  return <div className="setting-item no-border column hotkey-list-editor">
    <HotkeyBinding label="Global hotkey" value="" placeholder="+ Global hotkey" recordingLabel="Press a shortcut"
      recording={false} busy={false} disabled={false} onClick={undefined} onKeyDown={undefined}
      onMouseDown={undefined} onBlur={undefined} />
    <HotkeyList values={values} emptyLabel="Not set" removeLabel="Delete"
      onRemove={value => setValues(current => current.filter(binding => binding !== value))} />
  </div>;
}

export function EmojiFavoritesFixture() {
  return <div className="emoji-fav-grid">
    <EmojiAddTile label="Add emoji" onAdd={() => undefined} />
    <EmojiFavoriteTile name="Example image" removeLabel="Delete favorite" removeVisible
      onRevealRemove={() => undefined} onRemove={() => undefined} onChoose={() => undefined}>
      <span>🌿</span>
    </EmojiFavoriteTile>
  </div>;
}

export function SettingsDataFixture() {
  return <div className="showcase-fields">
    <SettingsDataPath label="Data storage path" path="C:/Example/Documents/Clipboard/profile.db">
      <SettingsDataAction>Change</SettingsDataAction><SettingsDataAction>Open</SettingsDataAction>
    </SettingsDataPath>
    <div className="data-actions-row">
      <SettingsDataAction icon="export">Export data</SettingsDataAction>
      <SettingsDataAction icon="import">Import data</SettingsDataAction>
    </div>
  </div>;
}

export function SettingsRemoveFixture() {
  return <div className="showcase-fields">
    <div className="settings-background-controls">
      <span className="item-label">Custom background</span>
      <SettingsRemoveButton label="Remove background" iconOnly />
    </div>
    <div className="settings-background-controls">
      <span className="item-label">Custom theme</span>
      <SettingsRemoveButton label="Remove theme" />
    </div>
  </div>;
}

export function TagToolbarFixture() {
  return <div className="tag-manager-content">
    <TagToolbar>
      <span className="tag-manager-current"><Tag size={14} /><span>Work</span></span>
      <button className="btn-icon" type="button" aria-label="Compact view"><ListCollapse size={16} /></button>
      <button className="btn-icon active" type="button" aria-label="Reveal entries"><Eye size={16} /></button>
      <button className="btn-icon" type="button" aria-label="Manage"><ListChecks size={16} /></button>
      <button className="btn-icon" type="button" aria-label="Add entry"><Plus size={16} /></button>
      <button className="btn-icon" type="button" aria-label="Tag actions"><Ellipsis size={16} /></button>
    </TagToolbar>
  </div>;
}
