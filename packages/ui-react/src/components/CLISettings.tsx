import { FolderOpen } from "lucide-react";
import { SettingHelp } from "./SettingHelp";
import { SettingsField } from "./SettingsField";
import { SettingsStatusBadge } from "./SettingsCapture";

interface CLIField {
  label: string;
  value: string;
  path?: boolean;
}

interface CLIGuide {
  label: string;
  path: string;
  fallback: string;
  disabled?: boolean;
  onOpen: () => void;
}

interface CLIStatus {
  label: string;
  value: string;
  available: boolean;
  action?: {
    label: string;
    disabled?: boolean;
    run: () => void;
  };
}

interface CLISettingsProps {
  title: string;
  description: string;
  helpLabel: string;
  fields: readonly CLIField[];
  status: CLIStatus;
  conflict?: readonly CLIField[];
  notice?: string;
  guidesTitle: string;
  guidesDescription: string;
  guideOpenLabel: string;
  guides: readonly CLIGuide[];
}

function ToolsHeading({ label, description, helpLabel }: { label: string; description: string; helpLabel: string }) {
  return <SettingsField className="settings-section-title">
    <div className="item-label-group">
      <div className="settings-label-line">
        <span className="item-label">{label}</span>
        <SettingHelp label={label} helpLabel={helpLabel}>{description}</SettingHelp>
      </div>
    </div>
  </SettingsField>;
}

function ToolValue({ label, value, path }: CLIField) {
  return <SettingsField detail>
    <div className="item-label-group"><span className="item-label">{label}</span></div>
    <span className={path ? "tools-path-text" : "tools-note"} title={path ? value : undefined}>{value}</span>
  </SettingsField>;
}

export function CLISettings({ title, description, helpLabel, fields, status, conflict, notice,
  guidesTitle, guidesDescription, guideOpenLabel, guides }: CLISettingsProps) {
  return <>
    <ToolsHeading label={title} description={description} helpLabel={helpLabel} />
    {fields.map(field => <ToolValue key={field.label} {...field} />)}
    <SettingsField detail className="tools-action-row">
      <div className="item-label-group"><span className="item-label">{status.label}</span></div>
      <div className="tools-path-status-actions">
        <span role="status"><SettingsStatusBadge label={status.value} available={status.available} /></span>
        {status.action && <button type="button" className="btn-icon settings-action settings-action--compact"
          disabled={status.action.disabled} onClick={status.action.run}>{status.action.label}</button>}
      </div>
    </SettingsField>
    {conflict?.map(field => <ToolValue key={field.label} {...field} />)}
    {notice && <SettingsField detail className="tools-note-item">
      <span className="tools-note" role="status">{notice}</span>
    </SettingsField>}
    <ToolsHeading label={guidesTitle} description={guidesDescription} helpLabel={helpLabel} />
    {guides.map(guide => <SettingsField key={guide.fallback} detail className="tools-action-row">
      <div className="item-label-group">
        <span className="item-label">{guide.label}</span>
        <span className="tools-path-text" title={guide.path || undefined}>{guide.path || guide.fallback}</span>
      </div>
      <button type="button" className="btn-icon settings-action settings-action--compact"
        disabled={!guide.path || guide.disabled} onClick={guide.onOpen} title={guideOpenLabel} aria-label={`${guideOpenLabel}: ${guide.label}`}>
        <FolderOpen size={14} aria-hidden="true" />
      </button>
    </SettingsField>)}
  </>;
}
