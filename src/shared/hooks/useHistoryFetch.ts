import { useCallback, useEffect, useRef, useState } from "react";
import { beginPrivacyAccess, invokePrivacyBound, subscribePrivacyInvalidation } from "../lib/privacySession";
import type { Dispatch, SetStateAction } from "react";
import type { ClipboardEntry } from "../types";
import { HISTORY_SEARCH_LIMIT } from "../../features/app/constants/pagination";
import { reuseUnchangedEntries } from "../lib/reuseUnchanged";
import { parseTagFilter } from "../lib/tagQuery";
import type { TagItemsPage } from "../../features/tag/types";

interface UseHistoryFetchOptions {
  debouncedSearch: string;
  typeFilter: string | null;
  persistentLimitEnabled: boolean;
  persistentLimit: number;
  pageSize: number;
  currentOffset: number;
  historyLength: number;
  setHistory: Dispatch<SetStateAction<ClipboardEntry[]>>;
  setCurrentOffset: Dispatch<SetStateAction<number>>;
  setHasMore: Dispatch<SetStateAction<boolean>>;
  isLoadingMore: boolean;
  hasMore: boolean;
  setIsLoadingMore: Dispatch<SetStateAction<boolean>>;
}

export const useHistoryFetch = ({
  debouncedSearch,
  typeFilter,
  pageSize,
  currentOffset,
  historyLength,
  setHistory,
  setCurrentOffset,
  setHasMore,
  isLoadingMore,
  hasMore,
  setIsLoadingMore
}: UseHistoryFetchOptions) => {
  const loadingRef = useRef(false);
  const fetchSeqRef = useRef(0);
  const lastRequestedOffsetRef = useRef<number | null>(null);
  const currentOffsetRef = useRef(currentOffset);
  const historyLengthRef = useRef(historyLength);
  const [appliedSearch, setAppliedSearch] = useState<string | null>(null);
  useEffect(() => subscribePrivacyInvalidation((scope) => {
    fetchSeqRef.current += 1;
    lastRequestedOffsetRef.current = null;
    if (scope === "all") setAppliedSearch(null);
  }), []);

  useEffect(() => {
    currentOffsetRef.current = currentOffset;
  }, [currentOffset]);

  useEffect(() => {
    historyLengthRef.current = historyLength;
  }, [historyLength]);
  const fetchHistory = useCallback(
    async (reset = false) => {
      const seq = ++fetchSeqRef.current;
      try {
        if (reset) {
          lastRequestedOffsetRef.current = null;
          setAppliedSearch(null);
        }

        const baseOffset = reset
          ? 0
          : Math.min(currentOffsetRef.current, historyLengthRef.current);

        let data: ClipboardEntry[] = [];

        const hasSearch = debouncedSearch && debouncedSearch.trim().length > 0;

        const selectedTag = hasSearch ? parseTagFilter(debouncedSearch) : null;
        if (selectedTag !== null) {
          const access = beginPrivacyAccess();
          if (!access) throw new Error("PrivacyStateUnavailable");
          const page = selectedTag ? await invokePrivacyBound<TagItemsPage>("get_tag_items_page", {
            tag: selectedTag, query: "", sortBy: "time", limit: Math.min(pageSize, 100), offset: baseOffset, expectedEpoch: access.epoch
          }) : { items: [], has_more: false, next_offset: null };
          if (seq !== fetchSeqRef.current) return false;
          setHistory(previous => reset ? reuseUnchangedEntries(previous, page.items) : [
            ...previous, ...page.items.filter(item => !previous.some(existing => existing.id === item.id
              && existing.host_instance_id === item.host_instance_id))
          ]);
          setCurrentOffset(page.next_offset ?? baseOffset + page.items.length);
          setHasMore(page.has_more);
        } else if (hasSearch) {

          try {
            data = await invokePrivacyBound<ClipboardEntry[]>("search_clipboard_history", {
              searchTerm: debouncedSearch,
              limit: HISTORY_SEARCH_LIMIT
            });
          } catch (e) {
            console.error("Search failed, falling back", e);
            throw e;
          }

          if (seq !== fetchSeqRef.current) return false;
          // Search results are not paginated; always replace list and stop infinite loading.
          setHistory((prev) => reuseUnchangedEntries(prev, data));
          setCurrentOffset(data.length);
          setHasMore(false);
        } else {
          const requestedLimit = pageSize + 1; // Use standard page size for DB limit
          const rawData = await invokePrivacyBound<ClipboardEntry[]>("get_clipboard_history", {
            limit: requestedLimit,
            offset: baseOffset,
            content_type: typeFilter || undefined
          });

          if (seq !== fetchSeqRef.current) return false;

          const hasMoreNow = rawData.length > pageSize;
          const data = hasMoreNow ? rawData.slice(0, pageSize) : rawData;

          // Calculate how many DB items we actually retrieved (id > 0)
          // This is critical for the next offset to be correct
          const dbItemsCount = data.filter(item => item.id > 0).length;

          if (reset) {
            setHistory((prev) => reuseUnchangedEntries(prev, data));
            setCurrentOffset(dbItemsCount);
            setHasMore(hasMoreNow);
          } else {
            let nextItems: ClipboardEntry[] = [];
            setHistory((prev) => {
              const existingIds = new Set(prev.map((item) => item.id));
              nextItems = data.filter((item) => !existingIds.has(item.id) || item.id === 0);

              if (nextItems.length === 0) return prev;
              return [...prev, ...nextItems];
            });

            setCurrentOffset(prev => prev + dbItemsCount);
            // If we didn't add any NEW items but the backend says there are more,
            // it means the items we got were already in our list (maybe shifted due to sorting).
            // We should keep hasMore true so the user can try to load further.
            setHasMore(hasMoreNow);
          }
        }
        setAppliedSearch(debouncedSearch);
        return true;
      } catch (err) {
        if (seq !== fetchSeqRef.current) return false;
        console.error("无法获取历史记录", err);
        setHasMore(false);
        return false;
      }
    },
    [
      debouncedSearch,
      typeFilter,
      pageSize,
      setCurrentOffset,
      setHasMore,
      setHistory
    ]
  );

  const loadMoreHistory = useCallback(async () => {
    if (loadingRef.current || isLoadingMore || !hasMore || appliedSearch !== debouncedSearch) return;
    if (debouncedSearch.trim() && parseTagFilter(debouncedSearch) === null) return;

    const effectiveOffset = Math.min(currentOffsetRef.current, historyLengthRef.current);
    if (lastRequestedOffsetRef.current === effectiveOffset) return;
    lastRequestedOffsetRef.current = effectiveOffset;

    loadingRef.current = true;
    setIsLoadingMore(true);
    try {
      await fetchHistory(false);
    } finally {
      loadingRef.current = false;
      setIsLoadingMore(false);
    }
  }, [appliedSearch, debouncedSearch, fetchHistory, hasMore, isLoadingMore, setIsLoadingMore]);

  return { fetchHistory, loadMoreHistory, appliedSearch };
};

