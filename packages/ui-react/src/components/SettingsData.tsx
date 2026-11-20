import type { ComponentPropsWithRef, ReactNode } from "react";
import { Download, Upload } from "lucide-react";

export function SettingsDataPath({ label, path, children }: { label: string; path: string; children: ReactNode }) {
  return <div className="setting-item column no-border">
    <div className="data-path-header">
      <span className="item-label data-path-label">{label}</span>
      <div className="data-path-actions">{children}</div>
    </div>
    <div className="data-panel" title={path}>{path}</div>
  </div>;
}

export function SettingsDataAction({ icon, children, className = "", ...props }: ComponentPropsWithRef<"button"> & {
  icon?: "export" | "import";
}) {
  return <button type="button" className={["btn-icon btn-icon-data-action", className].filter(Boolean).join(" ")} {...props}>
    {icon === "export" && <Download size={14} />}
    {icon === "import" && <Upload size={14} />}
    <span>{children}</span>
  </button>;
}
