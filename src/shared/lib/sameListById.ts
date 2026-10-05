/**
 * Whether two copies of an id-keyed list carry the same data.
 *
 * Lists that come back from the backend are freshly allocated every time, so an
 * identity check reports every one of them as changed. Handing such a list to a
 * state setter is a state update even when nothing in it moved, which re-renders
 * whatever component holds it.
 */
const sameRecord = (a: Record<string, unknown>, b: Record<string, unknown>): boolean => {
    const aKeys = Object.keys(a);
    const bKeys = Object.keys(b);
    if (aKeys.length !== bKeys.length) return false;
    for (const key of aKeys) {
        if (!Object.is(a[key], b[key])) return false;
    }
    return true;
};

/**
 * Compare two id-keyed lists by content, in order.
 *
 * Order is part of the answer because these lists drive menus, where a
 * reordering is a visible change even when the same items are present. The ids
 * are unique per list, so a length check plus an id walk is enough to rule out
 * duplicates on either side.
 */
export const sameListById = <T extends { id: string }>(
    prev: readonly T[],
    next: readonly T[]
): boolean => {
    if (prev === next) return true;
    if (prev.length !== next.length) return false;
    for (let i = 0; i < prev.length; i += 1) {
        const a = prev[i];
        const b = next[i];
        if (a === b) continue;
        if (a.id !== b.id) return false;
        if (!sameRecord(a as unknown as Record<string, unknown>,
                        b as unknown as Record<string, unknown>)) return false;
    }
    return true;
};
