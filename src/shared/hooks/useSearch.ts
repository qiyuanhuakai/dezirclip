import { useState, useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useDebounce } from "./useDebounce";
import type { ClipboardEntry } from "../types";

export type SearchMode = "fts" | "fuzzy" | "regex";

const SEARCH_LIMIT = 50;
const FUZZY_THRESHOLD = 1;
const DEBOUNCE_MS = 300;

interface UseSearchReturn {
  query: string;
  mode: SearchMode;
  results: ClipboardEntry[];
  loading: boolean;
  error: string | null;
  recentSearches: string[];
  setQuery: (q: string) => void;
  setMode: (m: SearchMode) => void;
  execute: () => void;
  clearResults: () => void;
  fetchRecentSearches: () => void;
}

export const useSearch = (): UseSearchReturn => {
  const [query, setQueryState] = useState("");
  const [mode, setModeState] = useState<SearchMode>("fts");
  const [results, setResults] = useState<ClipboardEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [recentSearches, setRecentSearches] = useState<string[]>([]);

  const abortRef = useRef(0);

  const debouncedQuery = useDebounce(query, DEBOUNCE_MS);

  const executeSearch = useCallback(
    async (q: string, m: SearchMode) => {
      const trimmed = q.trim();
      if (!trimmed) {
        setResults([]);
        setLoading(false);
        setError(null);
        return;
      }

      const seq = abortRef.current + 1;
      abortRef.current = seq;

      setLoading(true);
      setError(null);

      try {
        let data: ClipboardEntry[];

        switch (m) {
          case "fts":
            data = await invoke<ClipboardEntry[]>("search_fts", {
              query: trimmed,
              limit: SEARCH_LIMIT,
            });
            break;
          case "fuzzy":
            data = await invoke<ClipboardEntry[]>("search_fuzzy", {
              query: trimmed,
              threshold: FUZZY_THRESHOLD,
              limit: SEARCH_LIMIT,
            });
            break;
          case "regex":
            data = await invoke<ClipboardEntry[]>("search_regex", {
              pattern: trimmed,
              limit: SEARCH_LIMIT,
            });
            break;
        }

        if (seq !== abortRef.current) return;

        setResults(data);
        setLoading(false);
      } catch (err) {
        if (seq !== abortRef.current) return;

        const msg = err instanceof Error ? err.message : String(err);
        setError(msg);
        setLoading(false);
      }
    },
    []
  );

  useEffect(() => {
    if (query.trim()) {
      executeSearch(debouncedQuery, mode);
    } else {
      setResults([]);
      setLoading(false);
      setError(null);
    }
  }, [debouncedQuery, mode, query, executeSearch]);

  const setQuery = useCallback((q: string) => {
    setQueryState(q);
  }, []);

  const setMode = useCallback((m: SearchMode) => {
    setModeState(m);
  }, []);

  const execute = useCallback(() => {
    executeSearch(query, mode);
  }, [query, mode, executeSearch]);

  const clearResults = useCallback(() => {
    setResults([]);
    setError(null);
  }, []);

  const fetchRecentSearches = useCallback(async () => {
    try {
      const history = await invoke<string[]>("get_search_history");
      setRecentSearches(history);
    } catch {
      setRecentSearches([]);
    }
  }, []);

  return {
    query,
    mode,
    results,
    loading,
    error,
    recentSearches,
    setQuery,
    setMode,
    execute,
    clearResults,
    fetchRecentSearches,
  };
};
