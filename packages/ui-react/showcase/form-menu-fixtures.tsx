import { useState, type CSSProperties } from "react";
import ThemedSelect from "../src/components/ThemedSelect";
import { EmojiFavoritesPanel } from "../src/components/EmojiFavoriteTiles";
import { BackupActions, BackupChoices, BackupDialog, BackupField, BackupPreview, BackupReplaceConfirmation, ContextMenuItem, ContextMenuSurface, EmojiAddTile, EmojiFavoriteTile, HotkeyBinding, HotkeyList, HotkeyListLayout, NumberField,
  PrivacyDetectionOptions, PrivacyPatternsField, PrivacyRuleCard, PrivacyRuleGrid, PrivacyRulesHeading, SourceExclusionField, SettingHelp, SettingsChoices, SettingsDataAction, SettingsDataPath, SettingsField, SettingsRemoveButton, SettingsSectionCard, TagBatchToolbar, TagPickerTrigger, TagSearchField, TagToolbar, TagToolbarTitle, TextArea, TextField } from "../src";
import { Ellipsis, EyeOff, ListChecks, ListCollapse, Plus } from "lucide-react";
import { settingsPaneColumns } from "../src/components/SettingsSectionCard";
import { UpdateAction, UpdateActions, UpdateProgress, UpdateVersionRow } from "../src/components/SettingsUpdate";

export function TextFieldsFixture() {
  return <div className="showcase-fields">
    <label>Entry name<TextField defaultValue="Release checklist" /></label>
    <label>Search terms<TextField placeholder="Filter by name" /></label>
    <label>Idle timeout<NumberField defaultValue="30" /></label>
    <label>Notes<TextArea rows={3} defaultValue={"First line\nSecond line"} /></label>
  </div>;
}

export function SettingsFieldsFixture() {
  const [font, setFont] = useState("example");
  return <div className="showcase-fields">
    <SettingsField stacked>
      <div className="item-label-group">
        <div className="settings-label-line">
          <label className="item-label" htmlFor="example-font">Interface font</label>
          <SettingHelp label="Interface font" helpLabel="Setting help">Use the theme font by default.</SettingHelp>
        </div>
      </div>
      <ThemedSelect inputId="example-font" value={font} onChange={setFont} width="100%" searchable
        options={[{ value: "", label: "Default font" }, { value: "example", label: "Example Sans" }]} />
    </SettingsField>
    <SettingsField>
      <label className="item-label item-label-group" htmlFor="example-timeout">Idle timeout</label>
      <NumberField id="example-timeout" className="showcase-setting-number" defaultValue="30" />
    </SettingsField>
  </div>;
}

const menuItems = ["Copy", "Text processing", "Privacy", "Delete"];
const items: Record<string, string[]> = {
  "Text processing": ["Uppercase", "Lowercase", "Trim spaces"],
  "Privacy": ["Ignore this source", "Protect this source", "Manage rules"],
};

export function ContextMenuFixture({ state }: { state: string }) {
  const [active, setActive] = useState(menuItems[0]);
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

export function SettingsChoicesFixture({ state }: { state: string }) {
  const [mode, setMode] = useState("light");
  const [channel, setChannel] = useState("preview");
  const narrow = state === "narrow";
  const disabled = state === "disabled";
  return <div className="showcase-fields" style={narrow ? { width: 168 } : undefined}>
    <span className="item-label">Color mode</span>
    <SettingsChoices label="Color mode" value={mode} onChange={setMode} disabled={disabled}
      options={[{ value: "system", label: narrow ? "Follow system appearance" : "System" },
        { value: "light", label: "Light" }, { value: "dark", label: "Dark" }]} />
    <span className="item-label">Update channel</span>
    <SettingsChoices label="Update channel" value={channel} onChange={setChannel} segmented disabled={disabled}
      options={[{ value: "stable", label: narrow ? "Stable releases" : "Stable" },
        { value: "preview", label: narrow ? "Preview releases" : "Preview" }]} />
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

export function EmojiFavoritesFixture({ state }: { state: string }) {
  const empty = state === "empty" || state === "drop";
  return <EmojiFavoritesPanel style={{ height: 220, flex: "none" }}
    emptyTitle={empty ? "No emoji yet" : undefined}
    hint={empty ? "Click or drag images to add quickly" : "Drag to add emoji"}
    dragging={state === "drop"} dropLabel="Drop images here">
    <EmojiAddTile label="Add emoji" onAdd={() => undefined} />
    {!empty && <EmojiFavoriteTile name="Example image" removeLabel="Delete favorite" removeVisible={state === "default"}
      onRevealRemove={() => undefined} onRemove={() => undefined} onChoose={() => undefined}>
      <span>🌿</span>
    </EmojiFavoriteTile>}
  </EmojiFavoritesPanel>;
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

export function BackupDialogFixture({ state }: { state: string }) {
  const replacing = state.startsWith("replace");
  const encryptedImport = state === "import-encrypted";
  const restoring = state === "import" || encryptedImport || replacing;
  const encrypted = ["encrypted", "error", "hover-submit", "hover-cancel"].includes(state);
  const [format, setFormat] = useState(encrypted ? "encrypted" : "json");
  const [mode, setMode] = useState(replacing ? "replace" : "merge");
  const [open, setOpen] = useState(true);
  const [confirmed, setConfirmed] = useState(false);
  const title = restoring ? "Import clipboard history" : "Export clipboard history";
  const path = state === "default" || state === "glass" ? "" : restoring ? "C:/Example/clipboard.json" : "C:/Example/clipboard.dzc";
  const close = () => setOpen(false);
  const dialog = open && <BackupDialog title={title} onClose={close}
    footer={<BackupActions cancel="Cancel" submit={title} disabled={!path || mode === "replace" && !confirmed}
      onCancel={close} onSubmit={() => undefined} />}>
    {restoring
      ? <BackupChoices label="Import mode" value={mode} onChange={setMode} options={[
        { value: "merge", label: "Merge entries" }, { value: "replace", label: "Replace existing entries" },
      ]} />
      : <BackupChoices label="Export format" value={format} onChange={setFormat} options={[
        { value: "json", label: "JSON (plain text)" }, { value: "encrypted", label: "Encrypted backup" },
      ]} />}
    {mode === "replace" && <BackupReplaceConfirmation label="Confirm replacing clipboard history"
      checked={confirmed} onChange={setConfirmed} />}
    {!restoring && format === "encrypted" && <>
      <BackupField label="Encryption passphrase" type="password" placeholder="••••••••••••"
        error={state === "error" ? "Passphrase must be at least 12 characters" : undefined} />
      <BackupField label="Confirm passphrase" type="password" placeholder="••••••••••••" />
    </>}
    <BackupField label="File path" value={path} readOnly browse="Browse" />
    {encryptedImport
      ? <BackupField label="Decryption passphrase" type="password" placeholder="••••••••••••" />
      : restoring && <BackupPreview label="Preview entries" entries={["Release checklist", "Example screenshot", "Draft notes"]} />}
  </BackupDialog>;
  return state === "glass" ? <><div className="showcase-backup-underlay"><SettingsDataFixture /></div>{dialog}</> : dialog;
}

export function SettingsUpdateFixture({ state }: { state: string }) {
  const installing = state === "installing";
  const unknown = state === "unknown-size";
  return <div className="showcase-fields">
    <UpdateVersionRow label="Installed version" version="0.2.0-beta.4" />
    <UpdateVersionRow label="Channel version" version="0.2.0-beta.5" />
    <UpdateProgress label="Update download progress" status={installing ? "Installing and restarting…" : "Downloading update…"}
      amount={installing ? "100%" : unknown ? "2.5 MiB" : "42%"} percent={installing ? 100 : unknown ? null : 42}
      verified={installing ? "Official update signature verified" : undefined} />
  </div>;
}

export function UpdateActionsFixture({ state }: { state: string }) {
  const busy = state === "busy";
  const [clicked, setClicked] = useState("");
  return <div className="showcase-fields">
    <UpdateActions>
      <UpdateAction action="check" disabled={busy} onClick={() => setClicked("Checked")}>Check now</UpdateAction>
      <UpdateAction action="install" disabled={busy} onClick={() => setClicked("Install chosen")}>Install update</UpdateAction>
      <UpdateAction action="releases" onClick={() => setClicked("Release opened")}>Open release page</UpdateAction>
    </UpdateActions>
    {busy && <UpdateProgress label="Update download progress" status="Downloading update…" amount="42%" percent={42} />}
    {clicked && <span role="status">{clicked}</span>}
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
      <TagToolbarTitle label="Work" />
      <button className="btn-icon" type="button" aria-label="Compact view"><ListCollapse size={16} /></button>
      <button className="btn-icon active" type="button" aria-label="Hide entries"><EyeOff size={16} /></button>
      <button className="btn-icon" type="button" aria-label="Manage"><ListChecks size={16} /></button>
      <button className="btn-icon" type="button" aria-label="Add entry"><Plus size={16} /></button>
      <button className="btn-icon" type="button" aria-label="Tag actions"><Ellipsis size={16} /></button>
    </TagToolbar>
  </div>;
}

export function TagBatchToolbarFixture({ state }: { state: string }) {
  const [selected, setSelected] = useState(state !== "default");
  const narrow = state === "narrow";
  return <div style={{ width: narrow ? 205 : 360 }}>
    <TagBatchToolbar selectLabel={selected ? (narrow ? "Clear all matching entries" : "Clear selection") : "Select all"}
      countLabel={selected ? "32 selected" : "0 selected"} deleteLabel="Delete" allSelected={selected}
      deleteDisabled={!selected} onSelect={() => setSelected(value => !value)} onDelete={() => undefined} />
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

export function PrivacyRulesFixture({ resized }: { resized: boolean }) {
  const [confirming, setConfirming] = useState("scope");
  const exclusions = <div className="setting-item column source-exclusion-setting">
    <span className="item-label">Application exclusions</span>
    <SourceExclusionField aria-label="Excluded applications" defaultValue={"Example Editor\nExample Browser"} readOnly />
  </div>;
  return <div className="privacy-rule-manager" style={{ width: 490, "--settings-columns": 2 } as CSSProperties}>
    <PrivacyRulesHeading label="Privacy rules" count={2} />
    {resized && exclusions}
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
    {!resized && exclusions}
  </div>;
}

export function PrivacyDetectionFixture({ narrow }: { narrow: boolean }) {
  const [checked, setChecked] = useState(["phone", "email", "password"]);
  return <div style={{ width: narrow ? 290 : 490, display: "grid", gap: 8 }}>
    <div className="setting-item" style={{ display: "contents" }}><span className="item-label">Privacy detection</span></div>
    <PrivacyDetectionOptions options={[
      { value: "phone", label: "Phone numbers" }, { value: "id", label: "Identity numbers" },
      { value: "email", label: "Email addresses" }, { value: "secrets", label: "Secrets" },
      { value: "password", label: "Strong passwords" },
    ].map(option => ({ ...option, checked: checked.includes(option.value) }))}
      onChange={(value, enabled) => setChecked(values => enabled ? [...values, value] : values.filter(item => item !== value))} />
    <div className="setting-item" style={{ display: "contents" }}><span className="item-label">Custom detection patterns</span></div>
    <PrivacyPatternsField aria-label="Patterns" defaultValue={"EXAMPLE-[0-9]+\nproject_[a-z]+"}
      placeholder="One pattern per line" />
  </div>;
}

export function SearchSelectFixture({ state }: { state: string }) {
  const [font, setFont] = useState("Example Sans");
  const [app, setApp] = useState("");
  const narrow = state === "long-options";
  const code = state === "app-inherited" || state === "app-same-name";
  const appLabel = code ? "Code" : "Images";
  const fallback = code ? "Follow text setting (Example Editor)" : "System default (Example Browser)";
  return <div style={{ width: narrow ? 260 : 385, marginInlineStart: narrow ? 150 : 0, display: "grid", gap: 12 }}>
    <div className="setting-item column">
      <span className="item-label">Interface font</span>
      <ThemedSelect options={[{ value: "", label: "Default font" }, { value: "Example Sans", label: "Example Sans" },
        { value: "Example Serif", label: "Example Serif" }]} value={font} onChange={setFont} width="100%" searchable
        ariaLabel="Interface font" noOptionsMessage="No matching fonts" />
    </div>
    <div className="setting-item column">
      <span className="item-label" style={{ textTransform: "uppercase", fontSize: "11px", opacity: 0.8 }}>{appLabel}</span>
      <ThemedSelect options={[{ value: "", label: fallback },
        ...(state === "accented-app" ? [{ value: "/example/Café/Éditeur", label: "Café Éditeur" }] : []),
        ...(narrow ? [{ value: "/example/long", label: "Example Windows application with a very long display name for editing and browsing images" }] : []),
        ...(code ? [
          { value: "C:/Apps/CodeEditor.exe", label: "Code Editor (C:/Apps/CodeEditor.exe)" },
          { value: "D:/Portable/CodeEditor.exe", label: "Code Editor (D:/Portable/CodeEditor.exe)" },
        ] : [
          { value: "/example/photo-viewer", label: "Photo Viewer" }, { value: "/example/archive", label: "Archive Manager" },
        ])]}
        value={app} onChange={setApp} width="100%" searchable ariaLabel={appLabel} noOptionsMessage="No matching applications" />
    </div>
  </div>;
}
