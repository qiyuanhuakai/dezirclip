import type { ClipboardEntry } from "../types";

/**
 * Which entry the highlight belongs on.
 *
 * The quick paste panel keeps this as an id and turns it into a position on
 * every render, because every mutation of the list reorders it — pinned first,
 * then newest first. An index stored in state is stale the instant anything is
 * copied while the panel is open, and the highlight slides onto a different
 * entry; Enter then pastes whatever is highlighted, so the wrong thing gets
 * pasted. The two rules below are the whole of it, and they are kept here rather
 * than in the window so they can be pinned down by tests.
 */

/** Where `activeId` sits in the list right now, or the top when it is not there. */
export const resolveActiveIndex = (
    list: ClipboardEntry[],
    activeId: number | null
): number => {
    if (list.length === 0) return -1;
    if (activeId !== null) {
        const found = list.findIndex((entry) => entry.id === activeId);
        if (found >= 0) return found;
    }
    return 0;
};

/**
 * What to select after a `clipboard-updated` payload is folded in.
 *
 * A genuinely new entry is the one the user most likely wants, so the highlight
 * follows it wherever it sorts — including after a pinned entry, which is not
 * index 0. An edit of something already listed leaves the highlight on that same
 * entry; the list may have been reordered to put it elsewhere, and that is not a
 * reason to move the highlight.
 */
export const selectionAfterUpdate = (
    activeId: number | null,
    wasPresent: boolean,
    incoming: ClipboardEntry
): number | null => (wasPresent ? activeId : incoming.id);
