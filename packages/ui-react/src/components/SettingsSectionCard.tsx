import type { ReactNode } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";

export const PANE_TWO_COLUMNS_MIN_WIDTH = 700;
export const PANE_THREE_COLUMNS_MIN_WIDTH = 1000;

export function settingsPaneColumns(width: number): 1 | 2 | 3 {
  if (width >= PANE_THREE_COLUMNS_MIN_WIDTH) return 3;
  if (width >= PANE_TWO_COLUMNS_MIN_WIDTH) return 2;
  return 1;
}

export function SettingsGroupHeader({ title, collapsed, onToggle }: {
  title: ReactNode;
  collapsed: boolean;
  onToggle: () => void;
}) {
  return <div className="group-header" onClick={onToggle}>
    <h3 style={{ margin: 0 }}>{title}</h3>
    {collapsed ? <ChevronRight size={16} /> : <ChevronDown size={16} />}
  </div>;
}

export function SettingsSectionCard({ title, collapsed, onToggle, children }: {
  title: ReactNode;
  collapsed: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return <div className={"settings-group" + (collapsed ? " collapsed" : "")}>
    <SettingsGroupHeader title={title} collapsed={collapsed} onToggle={onToggle} />
    {!collapsed && <div className="group-content">{children}</div>}
  </div>;
}
