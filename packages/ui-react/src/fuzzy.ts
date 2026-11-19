import Fuse from "fuse.js";

export interface FuzzyResult<T = unknown> {
  item: T;
  score: number;
  matches?: ReadonlyArray<{ indices: ReadonlyArray<readonly [number, number]>; key?: string }>;
}

export interface FuzzyKey {
  name: string;
  weight: number;
}

export interface FuzzyIndexOptions {
  keys: FuzzyKey[];
  threshold?: number;
  ignoreLocation?: boolean;
  minMatchCharLength?: number;
}

const DEFAULT_INDEX_OPTIONS: Required<
  Pick<FuzzyIndexOptions, "threshold" | "ignoreLocation" | "minMatchCharLength">
> = {
  threshold: 0.35,
  ignoreLocation: true,
  minMatchCharLength: 2
};

export class FuzzyIndex<T> {
  private fuse: Fuse<T>;
  private options: FuzzyIndexOptions & {
    threshold: number;
    ignoreLocation: boolean;
    minMatchCharLength: number;
  };

  constructor(items: T[], options: FuzzyIndexOptions) {
    this.options = { ...DEFAULT_INDEX_OPTIONS, ...options };
    this.fuse = new Fuse(items, {
      keys: this.options.keys,
      threshold: this.options.threshold,
      ignoreLocation: this.options.ignoreLocation,
      minMatchCharLength: this.options.minMatchCharLength,
      includeScore: true,
      includeMatches: true
    });
  }

  setCollection(items: T[]): void {
    this.fuse.setCollection(items);
  }

  search(query: string, limit = 50): FuzzyResult<T>[] {
    const trimmed = query.trim();
    if (trimmed.length < this.options.minMatchCharLength) {
      return [];
    }
    const results = this.fuse.search(trimmed, { limit: limit * 2 });
    return results
      .filter((r) => r.score !== undefined && r.score <= this.options.threshold)
      .slice(0, limit)
      .map((r) => ({
        item: r.item,
        score: r.score ?? 1,
        matches: r.matches
      }));
  }
}

export interface FuzzyQuery {
  raw: string;
  terms: string[];
}

export function parseFuzzyQuery(query: string): FuzzyQuery {
  const trimmed = query.trim();
  if (!trimmed) return { raw: "", terms: [] };
  const terms = trimmed.split(/\s+/).filter(Boolean);
  return { raw: trimmed, terms };
}

export interface FuzzyFilterOptions {
  keys?: Array<string | FuzzyKey>;
  threshold?: number;
  minMatchCharLength?: number;
  limit?: number;
}

export function fuzzyFilter<T>(
  items: T[],
  query: string,
  getTarget?: (item: T) => string,
  options: FuzzyFilterOptions = {}
): FuzzyResult<T>[] {
  if (items.length === 0) return [];
  const trimmed = query.trim();
  if (trimmed.length === 0) {
    return items.map((item) => ({ item, score: 0 }));
  }
  if (!getTarget) {
    return fuzzyFilterInternal(items, trimmed, undefined, options);
  }
  return fuzzyFilterInternal(items, trimmed, getTarget, options);
}

function fuzzyFilterInternal<T>(
  items: T[],
  trimmed: string,
  getTarget: ((item: T) => string) | undefined,
  options: FuzzyFilterOptions
): FuzzyResult<T>[] {
  const threshold = options.threshold ?? 0.4;
  const minMatchCharLength = options.minMatchCharLength ?? 1;
  const limit = options.limit ?? items.length;

  if (getTarget) {
    const fuse = new Fuse(items, {
      keys: ["__text"],
      threshold,
      ignoreLocation: true,
      minMatchCharLength,
      includeScore: true,
      getFn: (item: T) => getTarget(item)
    });
    const results = fuse.search(trimmed, { limit: limit * 2 });
    return results
      .filter((r) => r.score !== undefined && r.score <= threshold)
      .slice(0, limit)
      .map((r) => ({ item: r.item as T, score: r.score ?? 1 }));
  }

  const fuse = new Fuse(items, {
    threshold,
    ignoreLocation: true,
    minMatchCharLength,
    includeScore: true
  });
  const results = fuse.search(trimmed, { limit: limit * 2 });
  return results
    .filter((r) => r.score !== undefined && r.score <= threshold)
    .slice(0, limit)
    .map((r) => ({ item: r.item as T, score: r.score ?? 1 }));
}
