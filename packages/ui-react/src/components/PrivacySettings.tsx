import "../styles/components/privacy.css";
import { useId, type ComponentPropsWithRef, type ReactNode } from "react";
import { TextArea } from "./TextField";
import { SettingsRemoveButton } from "./RemoveButton";
import { SettingsToggle } from "./SettingsToggle";

export function PrivacyRulesHeading({ label, count, id }: { label: string; count?: number; id?: string }) {
  return <div className="setting-item settings-section-title">
    <div className="item-label-group"><span id={id} className="item-label">{label}</span></div>
    {count !== undefined && <span className="settings-inline-note">{count}</span>}
  </div>;
}

export function PrivacyRuleGrid({ children }: { children: ReactNode }) {
  return <ul className="privacy-rule-grid">{children}</ul>;
}

export function PrivacyRuleCard({ title, source, scope, identifier, created, removeLabel, disabled, onRequestRemove, confirming }: {
  title: string;
  source: string;
  scope: string;
  identifier: ReactNode;
  created: ReactNode;
  removeLabel: string;
  disabled?: boolean;
  onRequestRemove: () => void;
  confirming?: {
    label: string;
    prompt: string;
    cancelLabel: string;
    onCancel: () => void;
    onConfirm: () => void;
  };
}) {
  return <li className="privacy-rule-card">
    <div className="privacy-rule-heading">
      <span className="item-label">{title}</span>
      <span className="privacy-rule-source">{source}</span>
    </div>
    <p className="privacy-rule-scope">{scope}</p>
    <div className="privacy-rule-metadata">
      {identifier}
      {created}
    </div>
    {confirming ? <div className="privacy-rule-confirm" role="group" aria-label={confirming.label}>
      <p>{confirming.prompt}</p>
      <div className="setting-btn-group">
        <button type="button" className="btn-icon settings-action" disabled={disabled} onClick={confirming.onCancel}>{confirming.cancelLabel}</button>
        <SettingsRemoveButton label={removeLabel} disabled={disabled} onClick={confirming.onConfirm} />
      </div>
    </div> : <button type="button" className="btn-icon settings-action" disabled={disabled} onClick={onRequestRemove}>{removeLabel}</button>}
  </li>;
}

export function SourceExclusionField({ className = "", ...props }: ComponentPropsWithRef<"textarea">) {
  return <TextArea className={["search-input source-exclusion-input", className].filter(Boolean).join(" ")} {...props} />;
}

export type PrivacyDetectionOption = { value: string; label: string; checked: boolean };

export function PrivacyDetectionOptions({ options, disabled, onChange }: {
  options: readonly PrivacyDetectionOption[];
  disabled?: boolean;
  onChange: (value: string, checked: boolean) => void;
}) {
  const id = useId();
  return <div className="privacy-detection-options">
    {options.map(option => {
      const inputId = `${id}-${option.value}`;
      return <div className="privacy-kind-option" key={option.value}>
        <SettingsToggle id={inputId} aria-label={option.label} checked={option.checked} disabled={disabled}
          onChange={event => onChange(option.value, event.currentTarget.checked)} />
        <label htmlFor={inputId} className="privacy-kind-label">{option.label}</label>
      </div>;
    })}
  </div>;
}

export function PrivacyPatternsField({ className = "", ...props }: ComponentPropsWithRef<"textarea">) {
  return <div className="privacy-patterns-field">
    <TextArea className={["search-input privacy-patterns-input", className].filter(Boolean).join(" ")} {...props} />
  </div>;
}
