import "../styles/components/filter-chip-controls.css";

export interface FilterChipsProps {
  readonly kinds: readonly { readonly id: string; readonly label: string }[];
  readonly activeKinds: readonly string[];
  readonly counts?: Record<string, number>;
  readonly allLabel: string;
  readonly clearLabel: string;
  readonly toggle: (id: string) => void;
  readonly clear: () => void;
  readonly set: (ids: string[]) => void;
}

export const FilterChips = ({ kinds, activeKinds, counts, allLabel, clearLabel, toggle, clear, set }: FilterChipsProps) => {
  return (
    <div className="filter-chips" data-test-filter-chips>
      <button
        className="filter-chip filter-chip--action"
        onClick={() => set(kinds.map((k) => k.id))}
        data-test-filter-all
      >
        {allLabel}
      </button>
      {kinds.map((kind) => (
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
        {clearLabel}
      </button>
    </div>
  );
};
