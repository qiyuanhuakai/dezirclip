import type { ComponentPropsWithRef, ReactNode } from "react";
import { ChevronDown, Search, Tag, X } from "lucide-react";

export function TagToolbar({ children }: { children: ReactNode }) {
  return <div className="tag-manager-toolbar">{children}</div>;
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
