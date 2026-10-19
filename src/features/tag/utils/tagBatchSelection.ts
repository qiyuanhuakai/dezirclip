export type TagBatchSelection =
    | { mode: 'selected'; ids: number[] }
    | { mode: 'all'; query: string; excludedIds: number[] };

export const MAX_TAG_BATCH_IDS = 10000;

function validEntryId(id: number): void {
    if (!Number.isSafeInteger(id) || id <= 0) {
        throw new RangeError('tag entry id must be a positive safe integer');
    }
}

export function emptyTagSelection(): TagBatchSelection {
    return { mode: 'selected', ids: [] };
}

export function selectAllTagEntries(query: string): TagBatchSelection {
    return { mode: 'all', query, excludedIds: [] };
}

export function tagEntrySelected(selection: TagBatchSelection, id: number): boolean {
    validEntryId(id);
    return selection.mode === 'selected' ? selection.ids.includes(id) : !selection.excludedIds.includes(id);
}

export function toggleTagEntry(selection: TagBatchSelection, id: number): TagBatchSelection {
    validEntryId(id);
    const current = selection.mode === 'selected' ? selection.ids : selection.excludedIds;
    const removing = current.includes(id);
    if (!removing && current.length >= MAX_TAG_BATCH_IDS) {
        throw new RangeError(`tag selection cannot contain more than ${MAX_TAG_BATCH_IDS} ids`);
    }
    const next = removing ? current.filter(existing => existing !== id) : [...current, id];
    return selection.mode === 'selected'
        ? { mode: 'selected', ids: next }
        : { mode: 'all', query: selection.query, excludedIds: next };
}

export function tagSelectionCount(selection: TagBatchSelection, total: number): number {
    return selection.mode === 'selected' ? selection.ids.length : Math.max(0, total - selection.excludedIds.length);
}
