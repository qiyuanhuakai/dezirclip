import type { ComponentPropsWithoutRef, ComponentPropsWithRef } from "react";
import "../styles/components/QuickPasteWindow.css";

export function QuickPastePanel({ className, ...props }: ComponentPropsWithoutRef<"div">) {
  return <div {...props} className={className ? `quick-paste-window ${className}` : "quick-paste-window"} />;
}

export function QuickPasteSearch(props: ComponentPropsWithRef<"input">) {
  return <input type="text" {...props} className="quick-paste-window__search-input" />;
}

type QuickPasteRowProps = Omit<ComponentPropsWithoutRef<"div">, "children"> & {
  preview: string;
  source: string;
  active?: boolean;
};

export function QuickPasteRow({ preview, source, active, ...props }: QuickPasteRowProps) {
  return <div {...props} className={active ? "quick-paste-window__item quick-paste-window__item--active" : "quick-paste-window__item"}>
    <div className="quick-paste-window__item-preview">{preview}</div>
    <div className="quick-paste-window__item-meta">{source}</div>
  </div>;
}
