import { forwardRef, useEffect, useState, useCallback, useRef, useMemo } from "react";
import { createPortal } from "react-dom";
import type { ClipboardEntry } from "../../../shared/types";

const SUBMENU_CLOSE_DELAY_MS = 120;

export interface TransformKindDto {
  id: string;
  label_zh: string;
  label_en: string;
}

export interface ItemContextMenuProps {
  x: number;
  y: number;
  entry?: ClipboardEntry | null;
  onSelect: (key: string) => void;
  onClose: () => void;
  onCopy?: () => void;
  onEditTags?: () => void;
  onQRCode?: () => void;
  onDelete?: () => void;
  onPin?: () => void;
  onShare?: () => void;
  onTransform?: (kind: string) => void;
  onOcr?: () => void;
  transformKinds?: TransformKindDto[];
  language?: "zh" | "en" | "tw";
  ocrRunning?: boolean;
}

const MENU_WIDTH = 180;
const MENU_ITEM_HEIGHT = 36;
const MENU_PADDING = 4;
const SUBMENU_MAX_HEIGHT = 320;
const SUBMENU_WIDTH = 168;
const SUBMENU_GAP = 4;

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

function submenuPosition(
  menuLeft: number,
  menuTop: number,
  itemIndex: number,
  itemCount: number
): { left: number; top: number } {
  const submenuHeight = Math.min(
    SUBMENU_MAX_HEIGHT,
    itemCount * MENU_ITEM_HEIGHT + MENU_PADDING * 2
  );
  const itemTop = menuTop + MENU_PADDING + itemIndex * MENU_ITEM_HEIGHT - MENU_PADDING;
  const rightLeft = menuLeft + MENU_WIDTH + SUBMENU_GAP;
  const leftLeft = menuLeft - SUBMENU_WIDTH - SUBMENU_GAP;
  const left = rightLeft + SUBMENU_WIDTH <= window.innerWidth - 8 ? rightLeft : Math.max(8, leftLeft);
  return clampPosition(left, itemTop, SUBMENU_WIDTH, submenuHeight);
}

const ItemContextMenu = forwardRef<HTMLDivElement, ItemContextMenuProps>(
  (
    {
      x,
      y,
      entry,
      onSelect,
      onClose,
      onCopy,
      onEditTags,
      onQRCode,
      onDelete,
      onPin,
      onShare,
      onTransform,
      onOcr,
      transformKinds = [],
      language = "zh",
      ocrRunning = false,
    },
    ref
  ) => {
    const [activeIndex, setActiveIndex] = useState(0);
    const [submenuOpen, setSubmenuOpen] = useState(false);
    const [submenuActiveIndex, setSubmenuActiveIndex] = useState(0);
    const internalRef = useRef<HTMLDivElement>(null);
    const resolvedRef = ref || internalRef;
    const submenuCloseTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    const clearSubmenuCloseTimer = useCallback(() => {
      if (submenuCloseTimerRef.current) {
        clearTimeout(submenuCloseTimerRef.current);
        submenuCloseTimerRef.current = null;
      }
    }, []);

    useEffect(() => {
      return () => {
        if (submenuCloseTimerRef.current) clearTimeout(submenuCloseTimerRef.current);
      };
    }, []);

    const menuItems = useMemo(() => {
      const ct = entry?.content_type;
      const isText = ct === "text" || ct === "rich_text" || ct === undefined;
      const isBinary = ct === "image" || ct === "video" || ct === "file";
      const isImage = ct === "image";
      const items: { key: string; label: string; hasSubmenu?: boolean }[] = [
        { key: "copy", label: "复制" },
        { key: "editTags", label: "编辑标签" },
      ];
      if (!isBinary) {
        items.push({ key: "qrCode", label: "QR 码" });
      }
      if (isImage) {
        items.push({ key: "ocr", label: ocrRunning ? "OCR 识别中..." : "OCR 识别" });
      }
      items.push({ key: "delete", label: "删除" });
      items.push({ key: "pin", label: entry?.is_pinned ? "取消固定" : "固定" });
      items.push({ key: "share", label: "分享" });
      if (isText) {
        items.push({ key: "transforms", label: "文本转换 →", hasSubmenu: true });
      }
      return items;
    }, [entry?.is_pinned, entry?.content_type, ocrRunning]);

	    const totalItems = menuItems.length;
	    const menuHeight = totalItems * MENU_ITEM_HEIGHT + MENU_PADDING * 2;
	    const pos = clampPosition(x, y, MENU_WIDTH, menuHeight);
	    const transformIndex = menuItems.findIndex((item) => item.key === "transforms");
	    const submenuPos = transformIndex >= 0
	      ? submenuPosition(pos.left, pos.top, transformIndex, transformKinds.length)
	      : { left: pos.left + MENU_WIDTH + SUBMENU_GAP, top: pos.top };

    const handleAction = useCallback(
      (key: string) => {
        onSelect(key);
        switch (key) {
          case "copy":
            onCopy?.();
            break;
          case "editTags":
            onEditTags?.();
            break;
          case "qrCode":
            onQRCode?.();
            break;
          case "delete":
            onDelete?.();
            break;
          case "pin":
            onPin?.();
            break;
          case "share":
            onShare?.();
            break;
          case "ocr":
            onOcr?.();
            break;
        }
        if (key !== "ocr") {
          onClose();
        }
      },
      [onSelect, onCopy, onEditTags, onQRCode, onDelete, onPin, onShare, onOcr, onClose]
    );

    const handleTransformSelect = useCallback(
      (kind: string) => {
        onSelect("transforms");
        onTransform?.(kind);
        onClose();
      },
      [onSelect, onTransform, onClose]
    );

    const handleKeyDown = useCallback(
      (e: KeyboardEvent) => {
        switch (e.key) {
          case "ArrowDown":
            e.preventDefault();
            if (submenuOpen) {
              setSubmenuActiveIndex(
                (prev) => (prev + 1) % Math.max(transformKinds.length, 1)
              );
            } else {
              setActiveIndex((prev) => (prev + 1) % totalItems);
            }
            break;
          case "ArrowUp":
            e.preventDefault();
            if (submenuOpen) {
              setSubmenuActiveIndex(
                (prev) =>
                  (prev - 1 + Math.max(transformKinds.length, 1)) %
                  Math.max(transformKinds.length, 1)
              );
            } else {
              setActiveIndex(
                (prev) => (prev - 1 + totalItems) % totalItems
              );
            }
            break;
          case "ArrowRight":
            e.preventDefault();
            if (!submenuOpen && menuItems[activeIndex]?.hasSubmenu) {
              setSubmenuOpen(true);
              setSubmenuActiveIndex(0);
            }
            break;
          case "ArrowLeft":
            e.preventDefault();
            if (submenuOpen) {
              setSubmenuOpen(false);
            }
            break;
          case "Enter":
            e.preventDefault();
            if (submenuOpen && transformKinds.length > 0) {
              handleTransformSelect(
                transformKinds[submenuActiveIndex]?.id ?? ""
              );
            } else if (menuItems[activeIndex]?.hasSubmenu) {
              setSubmenuOpen(true);
              setSubmenuActiveIndex(0);
            } else {
              handleAction(menuItems[activeIndex]?.key ?? "");
            }
            break;
          case "Escape":
            e.preventDefault();
            onClose();
            break;
        }
      },
      [
        activeIndex,
        submenuOpen,
        submenuActiveIndex,
        totalItems,
        transformKinds,
        menuItems,
        handleAction,
        handleTransformSelect,
        onClose,
      ]
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

	    const transformSubmenu = submenuOpen && transformKinds.length > 0 && createPortal(
	      <div
	        className="item-context-menu__submenu item-context-menu__submenu--open"
	        style={{
	          left: submenuPos.left,
	          top: submenuPos.top,
	          width: SUBMENU_WIDTH,
	          maxHeight: SUBMENU_MAX_HEIGHT,
	        }}
	        data-testid="transform-submenu"
	        onMouseEnter={() => {
	          clearSubmenuCloseTimer();
	          setSubmenuOpen(true);
	        }}
	        onMouseLeave={() => {
	          submenuCloseTimerRef.current = setTimeout(() => {
	            setSubmenuOpen(false);
	          }, SUBMENU_CLOSE_DELAY_MS);
	        }}
	      >
	        {transformKinds.map((kind, tIdx) => (
	          <div
	            key={kind.id}
	            role="menuitem"
	            data-testid="transform-item"
	            data-test-transform-kind={kind.id}
	            className={`item-context-menu__submenu-item ${
	              tIdx === submenuActiveIndex
	                ? "item-context-menu__submenu-item--active"
	                : ""
	            }`}
	            onMouseEnter={() => setSubmenuActiveIndex(tIdx)}
	            onClick={(e) => {
	              e.stopPropagation();
	              handleTransformSelect(kind.id);
	            }}
	          >
	            {language === "en" ? kind.label_en : kind.label_zh}
	          </div>
	        ))}
	      </div>,
	      document.body
	    );

	    return (
	      <>
	      <div
        ref={resolvedRef}
        role="menu"
        tabIndex={0}
        data-test-item-context-menu
        className="item-context-menu"
        style={{
          left: pos.left,
          top: pos.top,
          width: MENU_WIDTH,
        }}
      >
        {menuItems.map((item, index) => (
          <div
            key={item.key}
            role="menuitem"
            data-test-context-menu-item
            data-test-action={item.key}
            className={`item-context-menu__item ${
              index === activeIndex ? "item-context-menu__item--active" : ""
            } ${item.hasSubmenu ? "item-context-menu__item--has-submenu" : ""}`}
            onMouseEnter={() => {
              clearSubmenuCloseTimer();
              setActiveIndex(index);
              if (item.hasSubmenu) {
                setSubmenuOpen(true);
                setSubmenuActiveIndex(0);
              } else {
                setSubmenuOpen(false);
              }
            }}
            onClick={() => {
              setActiveIndex(index);
              if (item.hasSubmenu) {
                setSubmenuOpen(true);
                setSubmenuActiveIndex(0);
              } else {
                handleAction(item.key);
              }
            }}
          >
            <span className="item-context-menu__item-label">{item.label}</span>
	            {item.hasSubmenu && (
	              <span className="item-context-menu__item-arrow">▶</span>
	            )}
	          </div>
	        ))}
	      </div>
	      {transformSubmenu}
	      </>
	    );
  }
);

ItemContextMenu.displayName = "ItemContextMenu";

export { ItemContextMenu };
