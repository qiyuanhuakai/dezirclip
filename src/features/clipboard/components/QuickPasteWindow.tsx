import { useState, useEffect, useRef, useCallback, useMemo, forwardRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useBackendAppearance } from "../../../shared/hooks/useBackendAppearance";
import { applyHistoryUpdate } from "../../../shared/lib/historyInsert";
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
  useBackendAppearance();

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

  const fetchRecent = useCallback(() => {
    invoke<ClipboardEntry[]>("get_clipboard_history", {
      limit: MAX_ENTRIES,
      offset: 0,
      contentType: null,
    })
      .then((data) => setEntries(data ?? []))
      .catch(() => setEntries([]));
  }, []);

  // Fetch recent entries on mount
  useEffect(() => {
    fetchRecent();
  }, [fetchRecent]);

  // Follow the same broadcasts the main list follows.
  //
  // This window is created once and then parked, so the fetch above runs for
  // the lifetime of the process rather than for the lifetime of the panel: the
  // list used to freeze at whatever the database held the first time the window
  // was built, and everything copied after that was invisible here until the
  // process restarted. The backend emits both events to every window, so
  // applying them is enough — no refetch per change.
  useEffect(() => {
    let disposed = false;
    const unlistenUpdated = listen<ClipboardEntry>("clipboard-updated", (event) => {
      if (disposed) return;
      setEntries((prev) => {
        const { entries, wasPresent } = applyHistoryUpdate(prev, event.payload);
        // A genuinely new entry is what the user most likely wants to paste, so
        // the highlight follows it. An edit of something already listed leaves
        // the highlight where it is rather than moving the cursor under it.
        if (!wasPresent) setActiveIndex(0);
        return entries.slice(0, MAX_ENTRIES);
      });
    });
    const unlistenRemoved = listen<number>("clipboard-removed", (event) => {
      if (disposed) return;
      setEntries((prev) => prev.filter((entry) => entry.id !== event.payload));
    });
    // The backend only pushes captures to the frontend while the main window is
    // on screen, so an entry copied while the app was parked never reaches this
    // panel as an event. Every time the panel is shown, one fetch settles it --
    // once per hotkey press, not once per clipboard change.
    const unlistenShown = listen("quick-paste-shown", () => {
      if (disposed) return;
      fetchRecent();
    });
    return () => {
      disposed = true;
      unlistenUpdated.then((f) => f());
      unlistenRemoved.then((f) => f());
      unlistenShown.then((f) => f());
    };
  }, [fetchRecent]);

  useEffect(() => {
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
