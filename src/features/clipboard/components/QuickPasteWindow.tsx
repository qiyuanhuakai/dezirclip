import { useState, useEffect, useRef, useCallback, useMemo, forwardRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { applyThemeClass, applyModeClass } from "../../../shared/lib/themeRuntime";
import type { ClipboardEntry } from "../../../shared/types";
import "./QuickPasteWindow.css";

const MAX_ENTRIES = 10;

const QuickPasteWindow = forwardRef<HTMLDivElement>(function QuickPasteWindow(
  _props,
  ref
) {
  const [entries, setEntries] = useState<ClipboardEntry[]>([]);
  const [activeIndex, setActiveIndex] = useState(0);
  const [searchQuery, setSearchQuery] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement | null>(null);

  const filtered = useMemo(() => {
    if (!searchQuery.trim()) return entries;
    const q = searchQuery.toLowerCase();
    return entries.filter(
      (e) =>
        e.preview.toLowerCase().includes(q) ||
        e.content.toLowerCase().includes(q)
    );
  }, [entries, searchQuery]);

  // Clamp activeIndex when filtered list shrinks
  useEffect(() => {
    setActiveIndex((prev) => {
      if (filtered.length === 0) return 0;
      return Math.min(prev, filtered.length - 1);
    });
  }, [filtered.length]);

  // Fetch recent entries on mount
  useEffect(() => {
    invoke<ClipboardEntry[]>("get_clipboard_history", {
      limit: MAX_ENTRIES,
      offset: 0,
      contentType: null,
    })
      .then((data) => setEntries(data ?? []))
      .catch(() => setEntries([]));
  }, []);

  // Apply theme from localStorage (shared across windows)
  useEffect(() => {
    const theme = localStorage.getItem("tiez_theme") || "mica";
    const colorMode = localStorage.getItem("tiez_color_mode") || "light";
    applyThemeClass(document.documentElement, document.body, theme);
    applyModeClass(
      document.documentElement,
      document.body,
      colorMode === "dark" ? "dark" : "light"
    );
    document.body.classList.add("quick-paste");
    inputRef.current?.focus();
  }, []);

  const handlePaste = useCallback(async (entryId: number) => {
    try {
      await invoke("paste_quick_paste_selection", { entryId });
      await invoke("hide_quick_paste");
    } catch {
      // paste failed — window stays open
    }
  }, []);

  // Merge forwarded ref with internal listRef
  const setListRef = useCallback(
    (node: HTMLDivElement | null) => {
      listRef.current = node;
      if (typeof ref === "function") {
        ref(node);
      } else if (ref) {
        (ref as React.MutableRefObject<HTMLDivElement | null>).current = node;
      }
    },
    [ref]
  );

  // Global keyboard handler
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setActiveIndex((prev) => Math.min(prev + 1, filtered.length - 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setActiveIndex((prev) => Math.max(prev - 1, 0));
      } else if (e.key === "Enter") {
        e.preventDefault();
        if (filtered[activeIndex]) {
          handlePaste(filtered[activeIndex].id);
        }
      } else if (e.key === "Escape") {
        e.preventDefault();
        invoke("hide_quick_paste");
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [activeIndex, filtered, handlePaste]);

  // Scroll active item into view
  useEffect(() => {
    const node = listRef.current;
    if (!node) return;
    const activeItem = node.children[activeIndex] as HTMLElement | undefined;
    if (activeItem && typeof activeItem.scrollIntoView === "function") {
      activeItem.scrollIntoView({ block: "nearest" });
    }
  }, [activeIndex]);

  return (
    <div className="quick-paste-window">
      <div className="quick-paste-window__search">
        <input
          ref={inputRef}
          className="quick-paste-window__search-input"
          type="text"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder="搜索..."
          data-testid="quick-paste-search"
        />
      </div>
      <div
        ref={setListRef}
        className="quick-paste-window__list"
        data-testid="quick-paste-list"
      >
        {filtered.length === 0 ? (
          <div
            className="quick-paste-window__empty"
            data-testid="quick-paste-empty"
          >
            暂无最近记录
          </div>
        ) : (
          filtered.map((entry, i) => (
            <div
              key={entry.id}
              className={`quick-paste-window__item${
                i === activeIndex ? " quick-paste-window__item--active" : ""
              }`}
              onMouseEnter={() => setActiveIndex(i)}
              onClick={() => handlePaste(entry.id)}
              data-testid="quick-paste-item"
            >
              <div className="quick-paste-window__item-preview">
                {entry.preview || entry.content || "—"}
              </div>
              <div className="quick-paste-window__item-meta">
                {entry.source_app}
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
});

export default QuickPasteWindow;
