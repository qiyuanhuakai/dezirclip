import type { ClipboardEntry } from "../types";

/**
 * Whether two refetched copies of an entry carry the same data.
 *
 * Arrays are compared element by element because the backend hands back a fresh
 * `tags` array on every read, so an identity check alone would call every entry
 * changed and defeat the whole point.
 */
const sameValue = (a: unknown, b: unknown): boolean => {
    if (Object.is(a, b)) return true;
    if (Array.isArray(a) && Array.isArray(b)) {
        if (a.length !== b.length) return false;
        for (let i = 0; i < a.length; i += 1) {
            if (!Object.is(a[i], b[i])) return false;
        }
        return true;
    }
    return false;
};

const sameEntry = (prev: ClipboardEntry, next: ClipboardEntry): boolean => {
    const prevKeys = Object.keys(prev) as (keyof ClipboardEntry)[];
    const nextKeys = Object.keys(next) as (keyof ClipboardEntry)[];
    if (prevKeys.length !== nextKeys.length) return false;
    for (const key of prevKeys) {
        if (!sameValue(prev[key], next[key])) return false;
    }
    return true;
};

/**
 * Hand back the previous object for every entry a refetch returned unchanged.
 *
 * A first-page refetch replaces the whole array with fresh objects. The rows
 * are memoised, but a memo boundary cannot bail out when the only thing that
 * changed is the identity of data that is byte-for-byte the same, so closing
 * the tag manager -- which refetches to pick up whatever it changed, even when
 * it changed nothing -- re-rendered every visible row for no visible reason.
 *
 * The data that comes out is the data that went in; only the identity of the
 * entries that did not change is preserved, so the rows that genuinely changed
 * still re-render. Session entries carry `id: 0` and cannot be told apart, so
 * they are passed through untouched.
 */
export const reuseUnchangedEntries = (
    prev: ClipboardEntry[],
    next: ClipboardEntry[]
): ClipboardEntry[] => {
    if (prev.length === 0) return next;

    const byId = new Map<number, ClipboardEntry>();
    for (const entry of prev) {
        if (entry.id > 0) byId.set(entry.id, entry);
    }
    if (byId.size === 0) return next;

    return next.map((entry) => {
        if (entry.id <= 0) return entry;
        const old = byId.get(entry.id);
        return old && sameEntry(old, entry) ? old : entry;
    });
};
