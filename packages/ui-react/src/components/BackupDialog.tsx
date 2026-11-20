import { useEffect, useId, useRef, type ComponentProps, type ReactNode } from "react";

export function BackupDialog({ title, busy = false, onClose, children, footer }: {
  title: string; busy?: boolean; onClose: () => void; children: ReactNode; footer: ReactNode;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  useEffect(() => {
    const element = dialog.current;
    if (!element) return;
    if (element.showModal) element.showModal();
    else element.open = true;
    return () => { if (element.close) element.close(); else element.open = false; };
  }, []);
  return <dialog ref={dialog} className="data-modal-overlay" aria-labelledby={titleId} aria-busy={busy}
    onClick={event => { if (!busy && event.target === event.currentTarget) onClose(); }}
    onCancel={event => { event.preventDefault(); if (!busy) onClose(); }}>
    <div className="data-modal-content">
      <h2 className="data-modal-title" id={titleId}>{title}</h2>
      <div className="data-modal-body">{children}</div>
      {footer}
    </div>
  </dialog>;
}

export function BackupChoices<T extends string>({ label, value, options, disabled, onChange }: {
  label: string; value: T; options: readonly { value: T; label: string }[];
  disabled?: boolean; onChange: (value: T) => void;
}) {
  const group = useId();
  return <fieldset className="data-form-group data-choice-group" disabled={disabled}>
    <legend className="data-label">{label}</legend>
    <div className="data-radio-group">
      {options.map(option => <label className="data-radio-item" key={option.value}>
        <input type="radio" name={group} checked={value === option.value} onChange={() => onChange(option.value)} />
        <span>{option.label}</span>
      </label>)}
    </div>
  </fieldset>;
}

export function BackupField({ label, browse, onBrowse, error, disabled, ...props }: ComponentProps<"input"> & {
  label: string; browse?: string; onBrowse?: () => void; error?: string;
}) {
  const id = useId();
  const field = <input {...props} id={id} className={browse ? "data-input data-input-path" : "data-input"}
    disabled={disabled} aria-invalid={Boolean(error)} aria-describedby={error ? id + "-error" : undefined} />;
  return <div className="data-form-group">
    <label className="data-label" htmlFor={id}>{label}</label>
    {browse ? <div className="data-path-row">
      {field}
      <button type="button" className="btn-icon btn-icon-export-browse" disabled={disabled} onClick={onBrowse}>{browse}</button>
    </div> : field}
    {error && <span className="data-error" id={id + "-error"}>{error}</span>}
  </div>;
}

export function BackupPreview({ label, entries }: { label: string; entries: readonly string[] }) {
  const title = useId();
  return <div className="data-form-group">
    <div className="data-label" id={title}>{label}</div>
    <ul className="data-preview-list" aria-labelledby={title}>
      {entries.map((entry, index) => <li className="data-preview-item" key={index} title={entry}>{entry}</li>)}
    </ul>
  </div>;
}

export function BackupReplaceConfirmation({ label, checked, disabled, onChange }: {
  label: string; checked: boolean; disabled?: boolean; onChange: (checked: boolean) => void;
}) {
  return <label className="data-replace-confirm">
    <input type="checkbox" checked={checked} disabled={disabled} onChange={event => onChange(event.target.checked)} />
    <span>{label}</span>
  </label>;
}

export function BackupActions({ cancel, submit, busy, disabled, onCancel, onSubmit }: {
  cancel: string; submit: string; busy?: boolean; disabled?: boolean;
  onCancel: () => void; onSubmit: () => void;
}) {
  return <div className="data-modal-actions">
    <button type="button" className="data-btn data-btn-cancel" onClick={onCancel} disabled={busy}>{cancel}</button>
    <button type="button" className="data-btn data-btn-primary" onClick={onSubmit} disabled={busy || disabled}>{submit}</button>
  </div>;
}
