import { useCallback } from "react";

interface SearchHistoryProps {
  items: string[];
  onSelect: (term: string) => void;
  className?: string;
}

export const SearchHistory = ({
  items,
  onSelect,
  className,
}: SearchHistoryProps) => {
  const handleClick = useCallback(
    (term: string) => {
      onSelect(term);
    },
    [onSelect]
  );

  if (items.length === 0) {
    return (
      <div
        className={`search-history${className ? ` ${className}` : ""}`}
        data-test-search-history
      >
        <div className="search-history__empty" data-test-history-empty>
          暂无搜索记录
        </div>
      </div>
    );
  }

  return (
    <div
      className={`search-history${className ? ` ${className}` : ""}`}
      data-test-search-history
    >
      {items.map((term) => (
        <button
          key={term}
          className="search-history__item"
          onClick={() => handleClick(term)}
          data-test-history-item
        >
          <span className="search-history__icon" aria-hidden="true">
            🕐
          </span>
          {term}
        </button>
      ))}
    </div>
  );
};
