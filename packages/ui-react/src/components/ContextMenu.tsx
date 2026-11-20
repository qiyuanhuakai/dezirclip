import type { ComponentPropsWithRef } from "react";

export interface ContextMenuSurfaceProps extends ComponentPropsWithRef<"div"> {
  submenu?: boolean;
}

export function ContextMenuSurface({ submenu, className = "", ...props }: ContextMenuSurfaceProps) {
  const surface = submenu ? "item-context-menu__submenu item-context-menu__submenu--open" : "item-context-menu";
  return <div role={submenu ? undefined : "menu"} className={[surface, className].filter(Boolean).join(" ")} {...props} />;
}

export interface ContextMenuItemProps extends ComponentPropsWithRef<"div"> {
  submenu?: boolean;
  active?: boolean;
  hasSubmenu?: boolean;
}

export function ContextMenuItem({ submenu, active, hasSubmenu, className = "", children, ...props }: ContextMenuItemProps) {
  const item = submenu ? "item-context-menu__submenu-item" : "item-context-menu__item";
  return <div role="menuitem" aria-haspopup={hasSubmenu ? "menu" : undefined}
    className={[item, active && item + "--active", hasSubmenu && item + "--has-submenu", className].filter(Boolean).join(" ")}
    {...props}>
    {children}
    {hasSubmenu && <span className="item-context-menu__item-arrow" aria-hidden="true">▶</span>}
  </div>;
}
