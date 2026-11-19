import { FilterChips as Chips } from "@deziro/ui-react/components/FilterChips";
import { useFilterChips, CONTENT_KINDS } from "../../../shared/hooks/useFilterChips";

export const FilterChips = ({ counts }: { counts?: Record<string, number> }) => {
  const filters = useFilterChips();
  return <Chips {...filters} kinds={CONTENT_KINDS} counts={counts} allLabel="全部" clearLabel="清除" />;
};
