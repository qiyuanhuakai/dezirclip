/**
 * Which branch a history filter run takes, decided from the two search
 * strings alone. Kept separate from the hook so the decision is testable
 * without a React tree.
 *
 * The distinction that matters for cost: only the fuzzy branch needs a
 * Fuse index over the whole history. Every other branch just filters and
 * sorts the entries it was handed.
 */
export type SearchPlan =
  /** The server already returned the narrowed list; sort it as-is. */
  | { kind: "server" }
  /** Nothing to match on; filter by type and reorder by the list order. */
  | { kind: "plain" }
  /** A term that parsed to nothing; filter by type and leave the order alone. */
  | { kind: "unordered" }
  /** `tag:foo` matches against the tag list, not the content. */
  | { kind: "tag"; term: string }
  /** The only branch that needs a fuzzy index over the whole history. */
  | { kind: "fuzzy"; terms: string[] };

export interface PlanSearchInput {
  /** The live contents of the search box. */
  search: string;
  /** The debounced snapshot the server query was issued with. */
  debouncedSearch: string;
}

export const planSearch = ({ search, debouncedSearch }: PlanSearchInput): SearchPlan => {
  if (debouncedSearch && debouncedSearch === search) {
    return { kind: "server" };
  }

  // The list in memory is the server's answer to `debouncedSearch`. Filtering
  // it again with the half-typed `search` re-runs a match over data that is
  // already narrowed, and the result is thrown away as soon as the debounce
  // lands and the real query returns. Filtering it also showed the user three
  // lists per keystroke: narrowed by the stale term, back to the server's, then
  // the new one. The fuzzy branch is the expensive one -- it is the only branch
  // that builds a Fuse index over the whole list -- so this is where that cost
  // was being paid for an answer nobody kept.
  //
  // A search that has not reached the server yet is a different case: the list
  // in memory is the full history, and narrowing it as the user types is the
  // responsive behaviour, so an empty `debouncedSearch` still filters locally.
  if (debouncedSearch) {
    return { kind: "server" };
  }

  const raw = search.toLowerCase();
  const isTagSearch = raw.startsWith("tag:");
  const effectiveSearch = isTagSearch ? raw.slice(4) : raw;

  if (!effectiveSearch) {
    return { kind: "plain" };
  }

  if (isTagSearch) {
    return { kind: "tag", term: effectiveSearch };
  }

  const terms = effectiveSearch.trim().split(/\s+/).filter(Boolean);
  if (terms.length === 0) {
    return { kind: "unordered" };
  }

  return { kind: "fuzzy", terms };
};

/** Whether this plan needs a Fuse index built over the whole history. */
export const planNeedsFuzzyIndex = (plan: SearchPlan): boolean => plan.kind === "fuzzy";

/**
 * Pinned first, then pinned order, then newest. Shared by the branches that
 * only reorder what they were already given.
 */
export const compareForList = (
  a: { is_pinned?: boolean; pinned_order?: number; timestamp: number },
  b: { is_pinned?: boolean; pinned_order?: number; timestamp: number }
): number => {
  if (a.is_pinned !== b.is_pinned) return a.is_pinned ? -1 : 1;
  if (a.is_pinned) {
    if ((a.pinned_order || 0) !== (b.pinned_order || 0)) {
      return (b.pinned_order || 0) - (a.pinned_order || 0);
    }
  }
  return b.timestamp - a.timestamp;
};

/**
 * Same, except that two unpinned entries are ranked by score before the
 * timestamp. Pinned entries still keep their own ordering and ignore score,
 * which is what the list has always done.
 */
export const compareForScore = (
  a: { item: { is_pinned?: boolean; pinned_order?: number; timestamp: number }; score: number },
  b: { item: { is_pinned?: boolean; pinned_order?: number; timestamp: number }; score: number }
): number => {
  if (a.item.is_pinned !== b.item.is_pinned) return a.item.is_pinned ? -1 : 1;
  if (a.item.is_pinned) {
    if ((a.item.pinned_order || 0) !== (b.item.pinned_order || 0)) {
      return (b.item.pinned_order || 0) - (a.item.pinned_order || 0);
    }
    return b.item.timestamp - a.item.timestamp;
  }
  if (a.score !== b.score) return b.score - a.score;
  return b.item.timestamp - a.item.timestamp;
};
