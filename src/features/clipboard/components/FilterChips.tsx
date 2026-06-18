import { useFilterChips, CONTENT_KINDS } from "../../../shared/hooks/useFilterChips";
import "./FilterChips.css";

interface FilterChipsProps {
  counts?: Record<string, number>;
}

export const FilterChips = ({ counts }: FilterChipsProps) => {
  const { activeKinds, toggle, clear, set } = useFilterChips();

  return (
    <div className="filter-chips" data-test-filter-chips>
      <button
        className="filter-chip filter-chip--action"
        onClick={() => set(CONTENT_KINDS.map((k) => k.id))}
        data-test-filter-all
      >
        全部
      </button>
      {CONTENT_KINDS.map((kind) => (
        <button
          key={kind.id}
          className={`filter-chip${activeKinds.includes(kind.id) ? " filter-chip--active" : ""}`}
          onClick={() => toggle(kind.id)}
          data-test-filter-chip
          data-test-kind={kind.id}
        >
          {kind.label}
          {counts?.[kind.id] !== undefined && (
            <span className="filter-chip__count">{counts[kind.id]}</span>
          )}
        </button>
      ))}
      <button
        className="filter-chip filter-chip--action"
        onClick={clear}
        data-test-filter-clear
      >
        清除
      </button>
    </div>
  );
};
