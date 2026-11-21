import type { ReactNode } from "react";
import { SettingsChoices, type SettingsChoice } from "./SettingsChoices";
import { SettingsRemoveButton } from "./RemoveButton";
import "../styles/components/theme-manager.css";

export function SettingsThemeManager({ label, builtinLabel, importedLabel, value, builtins, imported, disabled,
  importLabel, exportLabel, removeLabel, importDisabled, limitMessage, notice, error, onChange, onImport, onExport, onRemove }: {
  label: string;
  builtinLabel: string;
  importedLabel: string;
  value: string;
  builtins: readonly SettingsChoice<string>[];
  imported: readonly SettingsChoice<string>[];
  disabled?: boolean;
  importLabel: string;
  exportLabel: string;
  removeLabel: string;
  importDisabled?: boolean;
  limitMessage?: string;
  notice?: string;
  error?: string;
  onChange: (value: string) => void;
  onImport: () => void;
  onExport: () => void;
  onRemove: () => void;
}) {
  const selected = imported.find(option => option.value === value);
  return <section className="theme-manager" aria-label={label} aria-busy={disabled}>
    <div className="theme-manager-heading">
      <span className="item-label">{label}</span>
      <div className="theme-manager-actions">
        <button type="button" className="btn-icon settings-action" disabled={disabled || importDisabled} onClick={onImport}>{importLabel}</button>
      </div>
    </div>
    <SettingsChoices className="theme-choices" label={builtinLabel} value={value} options={builtins} disabled={disabled} onChange={onChange} />
    {imported.length > 0 && <SettingsChoices className="theme-custom-list" label={importedLabel} value={value}
      options={imported} disabled={disabled} onChange={onChange} />}
    {selected && <div className="theme-manager-actions" aria-label={selected.label}>
      <button type="button" className="btn-icon settings-action" disabled={disabled} onClick={onExport}>{exportLabel}</button>
      <SettingsRemoveButton label={removeLabel} disabled={disabled} onClick={onRemove} />
    </div>}
    {limitMessage && <p className="theme-manager-hint">{limitMessage}</p>}
    {error && <p className="theme-operation-error" role="alert">{error}</p>}
    {notice && <p className="theme-manager-hint" role="status">{notice}</p>}
  </section>;
}

export function SettingsBackground({ label, chooseLabel, clearLabel, filename, disabled, onChoose, onClear, children }: {
  label: string;
  chooseLabel: string;
  clearLabel: string;
  filename?: string;
  disabled?: boolean;
  onChoose: () => void;
  onClear: () => void;
  children?: ReactNode;
}) {
  return <div className="setting-item column no-border settings-background">
    <div className="settings-background-controls">
      <span className="item-label">{label}</span>
      <button type="button" className="btn-icon settings-action settings-action--compact settings-background-choose"
        disabled={disabled} onClick={onChoose}>{chooseLabel}</button>
      {filename && <SettingsRemoveButton iconOnly className="settings-background-clear" label={clearLabel} disabled={disabled} onClick={onClear} />}
    </div>
    {filename && <div className="settings-background-file">{filename}</div>}
    {children}
  </div>;
}
