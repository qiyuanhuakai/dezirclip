import { useMemo } from "react";
import type { ClipboardEntry } from "../types";
import { fuzzyMatch, parseFuzzyQuery } from "../lib/fuzzy";

interface UseFilteredHistoryOptions {
  history: ClipboardEntry[];
  debouncedSearch: string;
  search: string;
  typeFilter: string | null;
}

const fuzzyItem = (item: ClipboardEntry, term: string): number => {
  if (!term) return 0;
  const target = `${item.content} ${item.source_app} ${item.tags.join(" ")}`;
  return fuzzyMatch(term, target).score;
};

const tagPrefixItem = (item: ClipboardEntry, term: string): boolean =>
  item.tags?.some((tag) => tag.toLowerCase().includes(term)) ?? false;

export const useFilteredHistory = ({
  history,
  debouncedSearch,
  search,
  typeFilter
}: UseFilteredHistoryOptions) => {
  return useMemo(() => {
    const rawSearch = search.toLowerCase();
    const isTagSearch = rawSearch.startsWith("tag:");
    const effectiveSearch = isTagSearch ? rawSearch.slice(4) : rawSearch;
    const { terms } = parseFuzzyQuery(effectiveSearch);
    const shouldBypassLocalSearch = !!debouncedSearch && debouncedSearch === search;

    const filtered = history.filter((item) => {
      if (typeFilter && item.content_type !== typeFilter) {
        return false;
      }

      if (shouldBypassLocalSearch) {
        return true;
      }

      if (!effectiveSearch) return true;

      if (isTagSearch) {
        return tagPrefixItem(item, effectiveSearch);
      }

      if (terms.length === 0) return true;

      return terms.every((term) => fuzzyItem(item, term) > 0);
    });

    const searchActive = !isTagSearch && terms.length > 0 && !shouldBypassLocalSearch;

    return filtered
      .map((item) => ({
        item,
        score: searchActive
          ? terms.reduce((sum, term) => sum + fuzzyItem(item, term), 0)
          : 0
      }))
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
        if (searchActive && a.score !== b.score) {
          return b.score - a.score;
        }
        return b.item.timestamp - a.item.timestamp;
      })
      .map((entry) => entry.item);
  }, [history, debouncedSearch, search, typeFilter]);
};
