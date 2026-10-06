import { useState, useEffect, useRef, useCallback, useMemo, forwardRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useBackendAppearance } from "../../../shared/hooks/useBackendAppearance";
import { applyHistoryUpdate } from "../../../shared/lib/historyInsert";
import {
  resolveActiveIndex,
  selectionAfterUpdate,
} from "../../../shared/lib/quickPasteSelection";
import type { ClipboardEntry } from "../../../shared/types";
import "./QuickPasteWindow.css";

const MAX_ENTRIES = 10;

const QuickPasteWindow = forwardRef<HTMLDivElement>(function QuickPasteWindow(
  _props,
  ref
) {
  const [entries, setEntries] = useState<ClipboardEntry[]>([]);
  // The selection is an entry id, not a position. Every mutation of this list
  // reorders it -- pinned first, then newest first -- so an index goes stale the
  // moment anything is copied while the panel is open, and the highlight silently
  // slides onto a different entry. Enter pastes whatever is highlighted, so a
  // stale index pastes the wrong thing. An id survives the reorder.
  const [activeId, setActiveId] = useState<number | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement | null>(null);
  // A mirror of `entries` for the event listeners to read.
  //
  // The listeners are registered once and depend only on the stable
  // `fetchRecent`, so anything they close over is frozen at the first render:
  // reading `activeId` from there meant an update to an entry that was already
  // listed resolved the selection from that stale `null` and dropped the
  // highlight back to the top. The list has the same problem, and folding it
  // through a state updater hides it, because the updater sees the current
  // value. Reading the ref and writing both states from one snapshot fixes both,
  // and keeps `setActiveId` out of another setter's updater, where React is free
  // to call it more than once.
  const entriesRef = useRef<ClipboardEntry[]>([]);
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

  // Derived, so there is no stored index that can disagree with the list. -1 only
  // when there is nothing to select; anything that drops the selected entry --
  // a removal, a search that filters it out, a refetch that no longer returns it
  // -- falls back to the top rather than leaving the highlight nowhere.
  const activeIndex = useMemo(
    () => resolveActiveIndex(filtered, activeId),
    [filtered, activeId]
  );

  const replaceEntries = useCallback((next: ClipboardEntry[]) => {
    entriesRef.current = next;
    setEntries(next);
  }, []);

  const fetchRecent = useCallback(() => {
    invoke<ClipboardEntry[]>("get_clipboard_history", {
      limit: MAX_ENTRIES,
      offset: 0,
      contentType: null,
    })
      .then((data) => replaceEntries(data ?? []))
      .catch(() => replaceEntries([]));
  }, [replaceEntries]);

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
      const incoming = event.payload;
      const { entries: merged, wasPresent } = applyHistoryUpdate(
        entriesRef.current,
        incoming
      );
      replaceEntries(merged.slice(0, MAX_ENTRIES));
      // A genuinely new entry is what the user most likely wants to paste, so
      // the highlight follows it. An edit of something already listed leaves
      // the highlight on the same entry rather than moving the cursor off it.
      // Either way this is an id, so a new entry that sorts after a pinned one
      // still ends up highlighted, and an edit that reorders the list does not
      // drag the highlight along with it. The functional form is what reads the
      // selection as it is now rather than as it was when this listener was
      // registered.
      setActiveId((current) => selectionAfterUpdate(current, wasPresent, incoming));
    });
    const unlistenRemoved = listen<number>("clipboard-removed", (event) => {
      if (disposed) return;
      replaceEntries(
        entriesRef.current.filter((entry) => entry.id !== event.payload)
      );
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
  }, [fetchRecent, replaceEntries]);

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
        const next = Math.min(activeIndex + 1, filtered.length - 1);
        if (filtered[next]) setActiveId(filtered[next].id);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        const next = Math.max(activeIndex - 1, 0);
        if (filtered[next]) setActiveId(filtered[next].id);
      } else if (e.key === "Enter") {
        e.preventDefault();
        const target = filtered[activeIndex];
        if (target) {
          handlePaste(target.id);
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
              onMouseEnter={() => setActiveId(entry.id)}
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
