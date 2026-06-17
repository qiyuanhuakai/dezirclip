import { forwardRef, useEffect, useState, useCallback, useRef } from "react";

export interface ItemContextMenuProps {
  x: number;
  y: number;
  onSelect: (index: number) => void;
  onClose: () => void;
}

const MENU_ITEMS = [
  { key: "copy", label: "复制" },
  { key: "editTags", label: "编辑标签" },
  { key: "transforms", label: "文本转换 \u2192" },
  { key: "delete", label: "删除" },
] as const;

const MENU_WIDTH = 180;
const MENU_ITEM_HEIGHT = 36;
const MENU_PADDING = 4;

function clampPosition(
  x: number,
  y: number,
  menuWidth: number,
  menuHeight: number
): { left: number; top: number } {
  const viewW = window.innerWidth;
  const viewH = window.innerHeight;
  let left = x;
  let top = y;
  if (left + menuWidth > viewW) left = viewW - menuWidth - 8;
  if (top + menuHeight > viewH) top = viewH - menuHeight - 8;
  if (left < 0) left = 8;
  if (top < 0) top = 8;
  return { left, top };
}

const ItemContextMenu = forwardRef<HTMLDivElement, ItemContextMenuProps>(
  ({ x, y, onSelect, onClose }, ref) => {
    const [activeIndex, setActiveIndex] = useState(0);
    const internalRef = useRef<HTMLDivElement>(null);
    const resolvedRef = ref || internalRef;

    const menuHeight = MENU_ITEMS.length * MENU_ITEM_HEIGHT + MENU_PADDING * 2;
    const pos = clampPosition(x, y, MENU_WIDTH, menuHeight);

    const handleKeyDown = useCallback(
      (e: KeyboardEvent) => {
        switch (e.key) {
          case "ArrowDown":
            e.preventDefault();
            setActiveIndex((prev) => (prev + 1) % MENU_ITEMS.length);
            break;
          case "ArrowUp":
            e.preventDefault();
            setActiveIndex(
              (prev) => (prev - 1 + MENU_ITEMS.length) % MENU_ITEMS.length
            );
            break;
          case "Enter":
            e.preventDefault();
            onSelect(activeIndex);
            break;
          case "Escape":
            e.preventDefault();
            onClose();
            break;
        }
      },
      [activeIndex, onSelect, onClose]
    );

    useEffect(() => {
      const el =
        typeof resolvedRef === "object" ? resolvedRef.current : null;
      el?.focus();
    }, [resolvedRef]);

    useEffect(() => {
      const el =
        typeof resolvedRef === "object" ? resolvedRef.current : null;
      if (!el) return;
      el.addEventListener("keydown", handleKeyDown);
      return () => el.removeEventListener("keydown", handleKeyDown);
    }, [handleKeyDown, resolvedRef]);

    return (
      <div
        ref={resolvedRef}
        role="menu"
        tabIndex={0}
        data-test-item-context-menu
        style={{
          position: "fixed",
          left: pos.left,
          top: pos.top,
          width: MENU_WIDTH,
          background: "var(--select-menu-bg)",
          border: "1px solid var(--select-menu-border)",
          boxShadow: "var(--select-menu-shadow)",
          borderRadius: "var(--surface-dialog-radius, 8px)",
          padding: `${MENU_PADDING}px 0`,
          outline: "none",
          zIndex: 9999,
          fontFamily: "var(--font-main)",
          fontSize: "var(--font-size-sm, 12px)",
        }}
      >
        {MENU_ITEMS.map((item, index) => (
          <div
            key={item.key}
            role="menuitem"
            data-test-context-menu-item
            onMouseEnter={() => setActiveIndex(index)}
            onClick={() => onSelect(index)}
            style={{
              padding: "8px 12px",
              cursor: "pointer",
              color:
                index === activeIndex
                  ? "var(--select-option-focus-color, var(--text-primary))"
                  : "var(--select-option-color, var(--text-primary))",
              background:
                index === activeIndex
                  ? "var(--select-option-focus-bg, var(--surface-history-selected-bg))"
                  : "transparent",
              transition: "background 0.1s ease",
            }}
          >
            {item.label}
          </div>
        ))}
      </div>
    );
  }
);

ItemContextMenu.displayName = "ItemContextMenu";

export { ItemContextMenu };
