import type { ComponentPropsWithoutRef, ReactNode } from "react";
import { Tag } from "lucide-react";

export function TagSuggestions({ label, count, children, ...props }: Omit<ComponentPropsWithoutRef<"div">, "className"> & {
  label: string;
  count: number;
  children: ReactNode;
}) {
  return <div {...props} className="dc-tag-filter-picker">
    <div className="dc-tag-filter-heading">
      <span><Tag size={13} aria-hidden="true" />{label}</span>
      <small>{count}</small>
    </div>
    {children}
  </div>;
}

export function TagSuggestion({ tag, color, active, ...props }: Omit<ComponentPropsWithoutRef<"button">, "className" | "children"> & {
  tag: string;
  color: string;
  active: boolean;
}) {
  return <div className="dc-tag-filter-row">
    <button {...props} type="button" role="option" aria-selected={active} tabIndex={-1}
      className={active ? "dc-tag-filter-option dc-tag-filter-active" : "dc-tag-filter-option"} data-tag={tag}>
      <span className="dc-tag-dot" style={{ backgroundColor: color }} />
      <span>{tag}</span>
    </button>
  </div>;
}

export function TagSuggestionStatus(props: Omit<ComponentPropsWithoutRef<"div">, "className">) {
  return <div {...props} className="dc-tag-filter-empty" />;
}

export function TagSuggestionRetry(props: Omit<ComponentPropsWithoutRef<"button">, "className">) {
  return <button {...props} type="button" className="dc-tag-filter-retry" />;
}
