import type { ClipboardEntry } from "../types";

/**
 * Put one entry where the history list expects it: pinned entries first, then
 * newest first. The list query orders by `is_pinned DESC, pinned_order DESC,
 * timestamp DESC, id DESC`, and this reproduces that order for the entries
 * already in hand so an incremental update lands in the same place a refetch
 * would have put it.
 *
 * It lives here rather than in either window because both the main list and the
 * quick paste panel apply the same rule to the same broadcast event, and two
 * copies of an ordering rule is one copy too many.
 */
export const insertHistoryItem = (
    list: ClipboardEntry[],
    item: ClipboardEntry
): ClipboardEntry[] => {
    const next = list.slice();
    const isPinned = !!item.is_pinned;
    let insertIndex = 0;

    if (isPinned) {
        while (insertIndex < next.length) {
            const current = next[insertIndex];
            if (!current.is_pinned) break;
            if (current.timestamp < item.timestamp) break;
            insertIndex++;
        }
    } else {
        while (insertIndex < next.length && next[insertIndex].is_pinned) {
            insertIndex++;
        }
        while (insertIndex < next.length) {
            const current = next[insertIndex];
            if (current.is_pinned) {
                insertIndex++;
                continue;
            }
            if (current.timestamp < item.timestamp) break;
            insertIndex++;
        }
    }

    next.splice(insertIndex, 0, item);
    return next;
};

/**
 * Fold a `clipboard-updated` payload into a list.
 *
 * An entry that is already in the list moves to its proper place instead of
 * being added twice, which is what the broadcast fires for a re-copy of
 * something already on screen. `wasPresent` is what the caller needs to decide
 * whether the highlight should move: a genuinely new entry is the one the user
 * most likely wants, an edit of a listed one is not.
 */
export const applyHistoryUpdate = (
    list: ClipboardEntry[],
    item: ClipboardEntry
): { entries: ClipboardEntry[]; wasPresent: boolean } => {
    const wasPresent = list.some((entry) => entry.id === item.id);
    return {
        entries: insertHistoryItem(
            list.filter((entry) => entry.id !== item.id),
            item
        ),
        wasPresent,
    };
};
