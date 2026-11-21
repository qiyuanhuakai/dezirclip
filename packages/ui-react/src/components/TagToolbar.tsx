import type { ComponentPropsWithoutRef, ComponentPropsWithRef, ReactNode } from "react";
import { ChevronDown, Search, Tag, X } from "lucide-react";

export function TagToolbar({ children }: { children: ReactNode }) {
  return <div className="tag-manager-toolbar">{children}</div>;
}

export function TagToolbarTitle({ label }: { label: string }) {
  return <span className="tag-manager-current" title={label}>
    <Tag size={14} aria-hidden="true" /><span>{label}</span>
  </span>;
}

export function TagBatchToolbar({ selectLabel, countLabel, deleteLabel, allSelected, selectDisabled, deleteDisabled, onSelect, onDelete }: {
  selectLabel: string; countLabel: string; deleteLabel: string; allSelected: boolean;
  selectDisabled?: boolean; deleteDisabled?: boolean; onSelect: () => void; onDelete: () => void;
}) {
  return <div className="tag-manager-batch-toolbar">
    <button type="button" className="btn-icon tag-manager-text-button" aria-pressed={allSelected}
      disabled={selectDisabled} onClick={onSelect}>{selectLabel}</button>
    <span role="status" aria-live="polite">{countLabel}</span>
    <button type="button" className="btn-icon tag-manager-text-button tag-manager-batch-delete"
      disabled={deleteDisabled} onClick={onDelete}>{deleteLabel}</button>
  </div>;
}

interface TagHistorySelection {
  label: string;
  checked: boolean;
  disabled?: boolean;
  onToggle: () => void;
}

export function TagHistoryEntry({ children, selection, onKeyDown, ...row }: ComponentPropsWithoutRef<"div"> & {
  selection?: TagHistorySelection;
}) {
  return <div {...row} className={selection ? "tag-history-row tag-history-row--managing" : "tag-history-row"}
    tabIndex={selection?.disabled ? -1 : 0}
    onClick={selection ? () => { if (!selection.disabled) selection.onToggle(); } : row.onClick}
    onContextMenu={selection ? event => { event.preventDefault(); event.stopPropagation(); } : row.onContextMenu}
    onKeyDown={event => {
      if (!selection) { onKeyDown?.(event); return; }
      if (selection.disabled || event.target !== event.currentTarget || (event.key !== "Enter" && event.key !== " ")) return;
      event.preventDefault();
      selection.onToggle();
    }}>
    {selection && <input type="checkbox" className="tag-history-selection" aria-label={selection.label}
      checked={selection.checked} disabled={selection.disabled} onClick={event => event.stopPropagation()}
      onChange={selection.onToggle} onKeyDown={event => {
        if (event.key !== "Enter") return;
        event.preventDefault();
        event.stopPropagation();
        selection.onToggle();
      }} />}
    <div className="tag-history-row-content" inert={Boolean(selection)}>{children}</div>
  </div>;
}

interface TagSearchFieldProps extends ComponentPropsWithRef<"input"> {
  variant: "contents" | "rail" | "picker";
  onClear?: () => void;
  clearLabel?: string;
}

export function TagSearchField({ variant, className, clearLabel, onClear, ...input }: TagSearchFieldProps) {
  const scope = variant === "contents" ? "tag-manager-search" : `tag-manager-${variant}-search`;
  return <div className={`search-container ${scope}`}>
    <Search className="search-icon" size={variant === "contents" ? 14 : 13} aria-hidden="true" />
    <input {...input} className={["search-input", className].filter(Boolean).join(" ")} />
    {Boolean(input.value) && !input.disabled && onClear && <button type="button" className="btn-icon tag-manager-clear"
      aria-label={clearLabel} onClick={onClear}><X size={13} aria-hidden="true" /></button>}
  </div>;
}

interface TagPickerTriggerProps extends ComponentPropsWithRef<"button"> {
  current: string;
}

export function TagPickerTrigger({ current, className, ...button }: TagPickerTriggerProps) {
  const expanded = button["aria-expanded"];
  return <button type="button" aria-haspopup="dialog" {...button}
    className={["btn-icon tag-manager-current tag-manager-picker-trigger", (expanded === true || expanded === "true") && "active", className].filter(Boolean).join(" ")}>
    <Tag size={14} aria-hidden="true" /><span>{current}</span><ChevronDown size={13} aria-hidden="true" />
  </button>;
}
