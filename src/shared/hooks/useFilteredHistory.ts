import { useMemo } from "react";
import type { ClipboardEntry } from "../types";
import { FuzzyIndex, parseFuzzyQuery } from "../lib/fuzzy";

interface UseFilteredHistoryOptions {
  history: ClipboardEntry[];
  debouncedSearch: string;
  search: string;
  typeFilter: string | null;
}

const buildSearchItem = (item: ClipboardEntry) => ({
  content: item.content ?? "",
  sourceApp: item.source_app ?? "",
  tagText: item.tags?.join(" ") ?? ""
});

export const useFilteredHistory = ({
  history,
  debouncedSearch,
  search,
  typeFilter
}: UseFilteredHistoryOptions) => {
  const searchItems = useMemo(() => history.map(buildSearchItem), [history]);
  const itemBySearchItem = useMemo(() => {
    const m = new Map<ReturnType<typeof buildSearchItem>, ClipboardEntry>();
    for (let i = 0; i < history.length; i++) {
      m.set(searchItems[i], history[i]);
    }
    return m;
  }, [history, searchItems]);

  const index = useMemo(
    () =>
      new FuzzyIndex(searchItems, {
        keys: [
          { name: "content", weight: 1 },
          { name: "sourceApp", weight: 0.6 },
          { name: "tagText", weight: 0.8 }
        ],
        threshold: 0.4,
        minMatchCharLength: 1
      }),
    [searchItems]
  );

  return useMemo(() => {
    const rawSearch = search.toLowerCase();
    const isTagSearch = rawSearch.startsWith("tag:");
    const effectiveSearch = isTagSearch ? rawSearch.slice(4) : rawSearch;
    const { terms } = parseFuzzyQuery(effectiveSearch);
    const shouldBypassLocalSearch = !!debouncedSearch && debouncedSearch === search;

    if (shouldBypassLocalSearch) {
      return history
        .filter((item) => !typeFilter || item.content_type === typeFilter)
        .sort((a, b) => {
          if (a.is_pinned !== b.is_pinned) return a.is_pinned ? -1 : 1;
          if (a.is_pinned) {
            if ((a.pinned_order || 0) !== (b.pinned_order || 0)) {
              return (b.pinned_order || 0) - (a.pinned_order || 0);
            }
            return b.timestamp - a.timestamp;
          }
          return b.timestamp - a.timestamp;
        });
    }

    if (!effectiveSearch) {
      return history
        .filter((item) => !typeFilter || item.content_type === typeFilter)
        .sort((a, b) => {
          if (a.is_pinned !== b.is_pinned) return a.is_pinned ? -1 : 1;
          if (a.is_pinned) {
            if ((a.pinned_order || 0) !== (b.pinned_order || 0)) {
              return (b.pinned_order || 0) - (a.pinned_order || 0);
            }
            return b.timestamp - a.timestamp;
          }
          return b.timestamp - a.timestamp;
        });
    }

    if (isTagSearch) {
      const filtered = history.filter(
        (item) =>
          (!typeFilter || item.content_type === typeFilter) &&
          (item.tags?.some((tag) => tag.toLowerCase().includes(effectiveSearch)) ?? false)
      );
      return filtered.sort((a, b) => b.timestamp - a.timestamp);
    }

    if (terms.length === 0) {
      return history.filter((item) => !typeFilter || item.content_type === typeFilter);
    }

    const scoreByItem = new Map<ClipboardEntry, number>();
    for (const term of terms) {
      const matches = index.search(term, searchItems.length);
      for (const m of matches) {
        const item = itemBySearchItem.get(m.item);
        if (item) {
          scoreByItem.set(item, (scoreByItem.get(item) ?? 0) + (1 - m.score));
        }
      }
    }

    const filtered = history.filter(
      (item) =>
        (!typeFilter || item.content_type === typeFilter) && scoreByItem.has(item)
    );

    return filtered
      .map((item) => ({ item, score: scoreByItem.get(item) ?? 0 }))
      .sort((a, b) => {
        if (a.item.is_pinned !== b.item.is_pinned) {
          return a.item.is_pinned ? -1 : 1;
        }
        if (a.item.is_pinned) {
          if ((a.item.pinned_order || 0) !== (b.item.pinned_order || 0)) {
            return (b.item.pinned_order || 0) - (a.item.pinned_order || 0);
          }
          return b.item.timestamp - a.item.timestamp;
        }
        if (a.score !== b.score) {
          return b.score - a.score;
        }
        return b.item.timestamp - a.item.timestamp;
      })
      .map((entry) => entry.item);
  }, [history, debouncedSearch, search, typeFilter, index, itemBySearchItem, searchItems]);
};
