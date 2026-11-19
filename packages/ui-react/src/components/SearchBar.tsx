import { useState, useRef, useEffect, useCallback } from "react";

export type SearchMode = "fts" | "fuzzy" | "regex";

const MODE_ICONS: Record<SearchMode, string> = { fts: "FTS5", fuzzy: "≈", regex: ".*" };
const MODE_LABELS: Record<SearchMode, string> = { fts: "FTS5", fuzzy: "模糊", regex: "正则" };

export interface SearchBarProps {
  query: string;
  mode: SearchMode;
  enabledModes: readonly SearchMode[];
  resultCount: number;
  loading: boolean;
  error: string | null;
  recentSearches: string[];
  setQuery: (query: string) => void;
  setMode: (mode: SearchMode) => void;
  fetchRecentSearches: () => void;
}

export const SearchBar = ({ query, mode, enabledModes, resultCount, loading, error,
  recentSearches, setQuery, setMode, fetchRecentSearches }: SearchBarProps) => {
  const [historyOpen, setHistoryOpen] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const wrapperRef = useRef<HTMLDivElement>(null);

  const handleClear = useCallback(() => {
    setQuery("");
    inputRef.current?.focus();
  }, [setQuery]);

  const handleHistorySelect = useCallback(
    (term: string) => {
      setQuery(term);
      setHistoryOpen(false);
    },
    [setQuery]
  );

  const handleFocus = useCallback(() => {
    fetchRecentSearches();
    setHistoryOpen(true);
  }, [fetchRecentSearches]);

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (wrapperRef.current && !wrapperRef.current.contains(e.target as Node)) {
        setHistoryOpen(false);
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  return (
    <div className="search-bar" data-test-search-bar>
      <div className="search-bar__row">
        <div className="search-bar__input-wrapper" ref={wrapperRef}>
          <span className="search-bar__icon" aria-hidden="true">
            🔍
          </span>
          <input
            ref={inputRef}
            className="search-bar__input"
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onFocus={handleFocus}
            placeholder="搜索剪贴板内容..."
            data-test-search-input
          />
          {query && (
            <button
              className="search-bar__clear"
              onClick={handleClear}
              aria-label="清除"
              data-test-search-clear
            >
              ×
            </button>
          )}
          {historyOpen && recentSearches.length > 0 && (
            <div className="search-bar__history" data-test-search-history>
              {recentSearches.map((term) => (
                <div
                  key={term}
                  className="search-bar__history-item"
                  onClick={() => handleHistorySelect(term)}
                  data-test-history-item
                >
                  <span className="search-bar__history-icon" aria-hidden="true">
                    🕐
                  </span>
                  {term}
                </div>
              ))}
            </div>
          )}
        </div>
        <div className="search-bar__mode" role="radiogroup" aria-label="搜索模式" title="模糊/正则搜索模式">
          {enabledModes.map((m) => (
            <button
              key={m}
              className={`search-bar__mode-btn${mode === m ? " search-bar__mode-btn--active" : ""}`}
              onClick={() => setMode(m)}
              role="radio"
              aria-checked={mode === m}
              aria-label={MODE_LABELS[m]}
              title={MODE_LABELS[m]}
              data-test-mode-btn
            >
              <span className="search-bar__mode-icon">{MODE_ICONS[m]}</span>
              {m !== "fts" && <span className="search-bar__mode-label">{MODE_LABELS[m]}</span>}
            </button>
          ))}
        </div>
      </div>
      {loading && (
        <div className="search-bar__loading" data-test-search-loading>
          <span className="search-bar__spinner" />
        </div>
      )}
      {error && (
        <div className="search-bar__error" data-test-search-error>
          {error}
        </div>
      )}
      {!loading && query && resultCount > 0 && (
        <div className="search-bar__count" data-test-search-count>
          找到 {resultCount} 条结果
        </div>
      )}
    </div>
  );
};
