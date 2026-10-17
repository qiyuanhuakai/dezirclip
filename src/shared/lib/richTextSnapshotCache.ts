/**
 * How many rich-text snapshots stay in memory.
 *
 * A snapshot is a base64 SVG data URL. The module rejects anything over 1.2 MB
 * per entry, so a cache of 120 entries has a ceiling well past 100 MB of
 * strings -- for an app whose whole pitch is staying small. That ceiling was
 * hard-coded, which left the user no way to trade memory for fewer
 * regenerations, so the limit became a setting and this file owns its range.
 *
 * The range deliberately includes 0 at the bottom. "Cache nothing" is the
 * setting a user reaches for when they want the memory back and do not mind
 * rebuilding a snapshot each time an item scrolls back into view.
 */

export const MIN_RICH_TEXT_SNAPSHOT_CACHE = 0;
export const MAX_RICH_TEXT_SNAPSHOT_CACHE = 500;

/** What the cache used before it was configurable; keeping it avoids a
 *  silent behaviour change for anyone who never touches the setting. */
export const DEFAULT_RICH_TEXT_SNAPSHOT_CACHE = 120;

/** Matches the slider step, so the reachable values are round numbers. */
export const RICH_TEXT_SNAPSHOT_CACHE_STEP = 20;

const NUMERIC = /^-?\d+$/;

/**
 * Coerce a stored setting into a usable limit.
 *
 * Anything unreadable -- missing key, blank string, a hand-edited value that is
 * not a whole number -- falls back to the default rather than to 0. Falling back
 * to 0 would read as "caching disabled" and silently cost the user the memory
 * setting they thought they had.
 *
 * The match is whole-string on purpose. `parseInt` would read `"12abc"` as 12
 * and `" 60 "` as 60, so a corrupted value would land on some plausible limit
 * nobody chose and the setting would stop meaning what it says.
 */
export const resolveRichTextSnapshotCacheLimit = (
    raw: string | null | undefined,
): number => {
    const trimmed = (raw ?? "").trim();
    if (!NUMERIC.test(trimmed)) return DEFAULT_RICH_TEXT_SNAPSHOT_CACHE;
    return Math.min(
        MAX_RICH_TEXT_SNAPSHOT_CACHE,
        Math.max(MIN_RICH_TEXT_SNAPSHOT_CACHE, Number.parseInt(trimmed, 10)),
    );
};

/**
 * How many entries have to be dropped to get back under `limit`, oldest first.
 *
 * Returned as a count rather than performed here so the Map stays owned by
 * the module that holds it, and so this is testable without one.
 */
export const snapshotEvictionCount = (size: number, limit: number): number =>
    Math.max(0, size - Math.max(0, limit));
