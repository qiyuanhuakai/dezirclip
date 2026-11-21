import { Camera } from "lucide-react";
import { SettingsField } from "./SettingsField";
import "../styles/components/capture-settings.css";

export function CaptureSettingsHeading({ label }: { label: string }) {
  return <SettingsField className="settings-section-title">
    <div className="item-label-group"><span className="item-label">{label}</span></div>
  </SettingsField>;
}

export function CaptureSettingsActions({ label, region, full, disabled, error, onRegion, onFull }: {
  label: string;
  region: string;
  full: string;
  disabled?: boolean;
  error?: string | null;
  onRegion: () => void;
  onFull: () => void;
}) {
  return <SettingsField detail className="settings-capture-actions">
    <div className="item-label-group"><span className="item-label">{label}</span></div>
    <div className="setting-btn-group">
      <button type="button" className="btn-icon settings-action" disabled={disabled} onClick={onRegion}><Camera size={14} />{region}</button>
      <button type="button" className="btn-icon settings-action" disabled={disabled} onClick={onFull}><Camera size={14} />{full}</button>
    </div>
    {error && <p role="alert" className="hotkey-recording-error">{error}</p>}
  </SettingsField>;
}

export function SettingsStatusBadge({ label, available }: { label: string; available: boolean }) {
  return <span className={`capture-engine-badge capture-engine-badge--${available ? "available" : "unavailable"}`}>
    <span className="capture-engine-badge__dot" aria-hidden="true" />
    {label}
  </span>;
}

export function CaptureEngineStatus({ label, status, available, retry, disabled, onRetry }: {
  label: string;
  status: string;
  available: boolean;
  retry: string;
  disabled?: boolean;
  onRetry: () => void;
}) {
  return <SettingsField className="settings-capture-status">
    <div className="item-label-group"><span className="item-label">{label}</span></div>
    <div className="tools-engine-status-actions">
      <span role="status"><SettingsStatusBadge label={status} available={available} /></span>
      <button type="button" className="btn-icon settings-action settings-action--compact" disabled={disabled} onClick={onRetry}>{retry}</button>
    </div>
  </SettingsField>;
}

export function FixedCaptureRenderer({ label, value }: { label: string; value: string }) {
  return <SettingsField detail className="quick-paste-renderer-setting">
    <div className="item-label-group"><span className="item-label">{label}</span></div>
    <span className="settings-value">{value}</span>
  </SettingsField>;
}
