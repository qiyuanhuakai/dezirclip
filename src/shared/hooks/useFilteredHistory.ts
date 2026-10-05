import { useMemo } from "react";
import type { ClipboardEntry } from "../types";
import { FuzzyIndex } from "../lib/fuzzy";
import { compareForList, compareForScore, planNeedsFuzzyIndex, planSearch } from "../lib/searchPlan";

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

type SearchItem = ReturnType<typeof buildSearchItem>;

const NO_SEARCH_ITEMS: SearchItem[] = [];
const NO_ITEM_LOOKUP: Map<SearchItem, ClipboardEntry> = new Map();

export const useFilteredHistory = ({
  history,
  debouncedSearch,
  search,
  typeFilter
}: UseFilteredHistoryOptions) => {
  const plan = useMemo(
    () => planSearch({ search, debouncedSearch }),
    [search, debouncedSearch]
  );

  // Building the Fuse index costs roughly a kilobyte of heap and a hundred
  // milliseconds per entry at large history sizes, and the branches below
  // are the common case: browsing, scrolling, and every clipboard capture
  // that appends a row. Only a live fuzzy query reads it back.
  const needsFuzzyIndex = planNeedsFuzzyIndex(plan);

  const searchItems = useMemo(
    () => (needsFuzzyIndex ? history.map(buildSearchItem) : NO_SEARCH_ITEMS),
    [history, needsFuzzyIndex]
  );

  const itemBySearchItem = useMemo(() => {
    if (!needsFuzzyIndex) return NO_ITEM_LOOKUP;
    const m: Map<SearchItem, ClipboardEntry> = new Map();
    for (let i = 0; i < history.length; i++) {
      m.set(searchItems[i], history[i]);
    }
    return m;
  }, [history, searchItems, needsFuzzyIndex]);

  const index = useMemo(() => {
    if (!needsFuzzyIndex) return null;
    return new FuzzyIndex(searchItems, {
      keys: [
        { name: "content", weight: 1 },
        { name: "sourceApp", weight: 0.6 },
        { name: "tagText", weight: 0.8 }
      ],
      threshold: 0.4,
      minMatchCharLength: 1
    });
  }, [searchItems, needsFuzzyIndex]);

  // The branches that only narrow and reorder what they were handed do not read
  // the plan, so they are memoised on the inputs they actually use. Keying them
  // on the plan meant every half-typed character -- which produces a fresh plan
  // object -- rebuilt the array and handed a new identity downstream, even
  // though the contents came out the same.
  const filteredByType = useMemo(
    () => (typeFilter ? history.filter((item) => item.content_type === typeFilter) : history),
    [history, typeFilter]
  );

  const listOrdered = useMemo(
    () => [...filteredByType].sort(compareForList),
    [filteredByType]
  );

  return useMemo(() => {
    const byType = (item: ClipboardEntry) =>
      !typeFilter || item.content_type === typeFilter;

    if (plan.kind === "server") {
      return listOrdered;
    }

    if (plan.kind === "plain") {
      return listOrdered;
    }

    if (plan.kind === "unordered") {
      return filteredByType;
    }

    if (plan.kind === "tag") {
      const term = plan.term;
      return history
        .filter(
          (item) =>
            byType(item) &&
            (item.tags?.some((tag) => tag.toLowerCase().includes(term)) ?? false)
        )
        .sort((a, b) => b.timestamp - a.timestamp);
    }

    // A fuzzy plan always comes with an index; the guard only narrows the type.
    if (!index) {
      return history.filter(byType);
    }

    const scoreByItem = new Map<ClipboardEntry, number>();
    for (const term of plan.terms) {
      const matches = index.search(term, searchItems.length);
      for (const m of matches) {
        const item = itemBySearchItem.get(m.item);
        if (item) {
          scoreByItem.set(item, (scoreByItem.get(item) ?? 0) + (1 - m.score));
        }
      }
    }

    return history
      .filter((item) => byType(item) && scoreByItem.has(item))
      .map((item) => ({ item, score: scoreByItem.get(item) ?? 0 }))
      .sort(compareForScore)
      .map((entry) => entry.item);
  }, [history, plan, typeFilter, index, itemBySearchItem, searchItems, filteredByType, listOrdered]);
};
