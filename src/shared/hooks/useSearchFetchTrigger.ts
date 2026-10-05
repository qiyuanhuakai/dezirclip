import { useEffect, useRef } from "react";

interface UseSearchFetchTriggerOptions {
  debouncedSearch: string;
  isComposing: boolean;
  typeFilter?: string | null;
  fetchHistory: (reset?: boolean) => void;
}

export const useSearchFetchTrigger = ({
  debouncedSearch,
  isComposing,
  typeFilter,
  fetchHistory
}: UseSearchFetchTriggerOptions) => {
  // Both effects exist to react to a change, and the app already asks for the
  // first page on mount. Firing on the initial value as well meant the page was
  // fetched four times over on every startup -- and once more on every wake
  // after the idle destroyer rebuilt the webview -- when one fetch returns the
  // same rows.
  //
  // The comparison is against the previously seen value rather than a run
  // counter, so React's development double-invocation of effects does not turn
  // the first run into a fetch on the replay.
  const prevSearchRef = useRef<string | null>(null);
  useEffect(() => {
    const previous = prevSearchRef.current;
    prevSearchRef.current = debouncedSearch;
    if (isComposing) return;
    if (previous === null || previous === debouncedSearch) return;
    fetchHistory(true);
  }, [debouncedSearch, isComposing, fetchHistory]);

  const prevTypeFilterRef = useRef<string | null | undefined>(undefined);
  useEffect(() => {
    const previous = prevTypeFilterRef.current;
    prevTypeFilterRef.current = typeFilter;
    if (previous === undefined || previous === typeFilter) return;
    fetchHistory(true);
  }, [typeFilter, fetchHistory]);
};
