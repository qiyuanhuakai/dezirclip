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
  // This effect reacts to changes; the app already asks for the
  // first page on mount. Firing on the initial value as well meant the page was
  // fetched four times over on every startup -- and once more on every wake
  // after the idle destroyer rebuilt the webview -- when one fetch returns the
  // same rows.
  //
  // The comparison is against the previously seen value rather than a run
  // counter, so React's development double-invocation of effects does not turn
  // the first run into a fetch on the replay.
  const previousRef = useRef<{ search: string; type: string | null | undefined } | null>(null);
  useEffect(() => {
    if (isComposing) return;
    const previous = previousRef.current;
    previousRef.current = { search: debouncedSearch, type: typeFilter };
    if (previous === null || (previous.search === debouncedSearch && previous.type === typeFilter)) return;
    fetchHistory(true);
  }, [debouncedSearch, isComposing, typeFilter, fetchHistory]);
};
