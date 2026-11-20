import { useState, type CSSProperties } from "react";
import { ContextMenuItem, ContextMenuSurface, EmojiAddTile, EmojiFavoriteTile, HotkeyBinding, HotkeyList, HotkeyListLayout, NumberField,
  PrivacyRuleCard, PrivacyRuleGrid, PrivacyRulesHeading, SourceExclusionField, SettingsChoices, SettingsDataAction, SettingsDataPath, SettingsRemoveButton, SettingsSectionCard, TagPickerTrigger, TagSearchField, TagToolbar, TextArea, TextField } from "../src";
import { Ellipsis, Eye, ListChecks, ListCollapse, Plus, Tag } from "lucide-react";
import { settingsPaneColumns } from "../src/components/SettingsSectionCard";

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

export function SettingsSectionGridFixture() {
  return <div className="settings-layout--split showcase-settings-grid">
    <div className={`settings-pane settings-pane--cols-${settingsPaneColumns(700)}`}>
      <SettingsSectionCard title="General settings" collapsed={false} onToggle={() => undefined}>
        <div className="item-label">Start hidden</div>
        <div className="item-label">Open near pointer</div>
        <div className="settings-row--wide"><TextField aria-label="Data path" defaultValue="C:/Example/Clipboard" /></div>
      </SettingsSectionCard>
    </div>
  </div>;
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

export function HotkeyListFixture({ multiple, winV, extended }: { multiple: boolean; winV: boolean; extended: boolean }) {
  const [values, setValues] = useState(extended ? ["Ctrl+1", "Ctrl+Numpad1", "Alt+Semicolon"]
    : multiple ? ["Super+V", "Ctrl+Shift+V"] : ["Super+V"]);
  return <HotkeyListLayout>
    <HotkeyBinding label="Global hotkey" value="" placeholder="+ Global hotkey" recordingLabel="Press a shortcut"
      recording={false} busy={false} disabled={false} onClick={undefined} onKeyDown={undefined}
      onMouseDown={undefined} onBlur={undefined} />
    <HotkeyList platform={winV ? "windows" : "linux"} values={values} emptyLabel="Not set" removeLabel="Delete"
      canRemove={value => !winV || value !== "Super+V"}
      onRemove={value => setValues(current => current.filter(binding => binding !== value))} />
  </HotkeyListLayout>;
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

export function TagSearchFixture() {
  const [query, setQuery] = useState("Release notes");
  return <div className="tag-manager" style={{ display: "grid", gap: 8, width: 380 }}>
    <TagToolbar><TagPickerTrigger current="Work" aria-expanded={false} /></TagToolbar>
    <div style={{ width: 156 }}><TagSearchField variant="rail" aria-label="Find a tag" placeholder="Find a tag" /></div>
    <TagSearchField variant="picker" aria-label="Find tags" placeholder="Find tags" />
    <TagSearchField variant="contents" aria-label="Search tagged entries" value={query}
      onChange={event => setQuery(event.target.value)} onClear={() => setQuery("")} clearLabel="Clear search" />
  </div>;
}

export function PrivacyRulesFixture() {
  const [confirming, setConfirming] = useState("scope");
  return <div className="privacy-rule-manager" style={{ width: 490, "--settings-columns": 2 } as CSSProperties}>
    <PrivacyRulesHeading label="Privacy rules" count={2} />
    <PrivacyRuleGrid>
      <PrivacyRuleCard title="Allowed content" source="Example Editor" scope="Applies only to matching content."
        identifier={<span>ID a13b4108</span>} created={<span>Created 2026-08-18 10:24</span>}
        removeLabel="Revoke exception" onRequestRemove={() => setConfirming("exact")}
        confirming={confirming === "exact" ? {
          label: "Remove privacy rule", prompt: "Revoke this exception?", cancelLabel: "Cancel",
          onCancel: () => setConfirming(""), onConfirm: () => setConfirming(""),
        } : undefined} />
      <PrivacyRuleCard title="Ignore source" source="Example Browser" scope="Applies to this application's future entries."
        identifier={<span>ID b27c5419</span>} created={<span>Created 2026-08-18 11:35</span>}
        removeLabel="Remove rule" onRequestRemove={() => setConfirming("scope")}
        confirming={confirming === "scope" ? {
          label: "Remove privacy rule", prompt: "Remove the rule for Example Browser?", cancelLabel: "Cancel",
          onCancel: () => setConfirming(""), onConfirm: () => setConfirming(""),
        } : undefined} />
    </PrivacyRuleGrid>
    <div className="setting-item column source-exclusion-setting">
      <span className="item-label">Excluded applications</span>
      <SourceExclusionField aria-label="Excluded applications" defaultValue={"Example Editor\nExample Browser"} readOnly />
    </div>
  </div>;
}
