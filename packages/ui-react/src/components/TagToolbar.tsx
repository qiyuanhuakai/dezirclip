import type { ReactNode } from "react";

export function TagToolbar({ children }: { children: ReactNode }) {
  return <div className="tag-manager-toolbar">{children}</div>;
}
