import { describe, expect, it } from 'vitest';
import {
    emptyTagSelection,
    MAX_TAG_BATCH_IDS,
    selectAllTagEntries,
    tagEntrySelected,
    tagSelectionCount,
    toggleTagEntry,
    type TagBatchSelection,
} from './tagBatchSelection';

describe('tag batch selection', () => {
    it('creates independent empty selections and starts without selected rows', () => {
        const first = emptyTagSelection();
        const second = emptyTagSelection();
        expect(first).toEqual({ mode: 'selected', ids: [] });
        expect(first).not.toBe(second);
        expect(tagEntrySelected(first, 1)).toBe(false);
        expect(tagSelectionCount(first, 100)).toBe(0);
        expect(toggleTagEntry(first, 1)).toEqual({ mode: 'selected', ids: [1] });
        expect(second).toEqual({ mode: 'selected', ids: [] });
    });

    it('toggles explicit selection twice without mutating frozen previous states', () => {
        const before: TagBatchSelection = { mode: 'selected', ids: [3, 7] };
        Object.freeze(before.ids);
        Object.freeze(before);
        const selected = toggleTagEntry(before, 99);
        expect(selected).not.toBe(before);
        expect(tagEntrySelected(selected, 99)).toBe(true);
        expect(before).toEqual({ mode: 'selected', ids: [3, 7] });
        expect(toggleTagEntry(selected, 99)).toEqual(before);
        expect(tagEntrySelected(toggleTagEntry(before, 3), 3)).toBe(false);
        expect(tagEntrySelected(before, 3)).toBe(true);
    });

    it('keeps a query snapshot and selects unseen pages without materializing their ids', () => {
        const query = '  设计\r\n参考  ';
        const all = selectAllTagEntries(query);
        expect(all).toEqual({ mode: 'all', query, excludedIds: [] });
        expect(tagEntrySelected(all, 1)).toBe(true);
        expect(tagEntrySelected(all, 90001)).toBe(true);
        expect(tagEntrySelected(all, Number.MAX_SAFE_INTEGER)).toBe(true);
        const excluded = toggleTagEntry(toggleTagEntry(all, 1), 90001);
        expect(excluded).toEqual({ mode: 'all', query, excludedIds: [1, 90001] });
        expect(tagEntrySelected(excluded, 1)).toBe(false);
        expect(tagEntrySelected(excluded, 90001)).toBe(false);
        expect(tagEntrySelected(excluded, 60)).toBe(true);
        expect(toggleTagEntry(toggleTagEntry(excluded, 1), 90001)).toEqual(all);
        expect(all).toEqual({ mode: 'all', query, excludedIds: [] });
    });

    it('does not mutate a frozen all-selection when exclusions change', () => {
        const before: TagBatchSelection = { mode: 'all', query: 'saved query', excludedIds: [7] };
        Object.freeze(before.excludedIds);
        Object.freeze(before);
        expect(toggleTagEntry(before, 9)).toEqual({ mode: 'all', query: 'saved query', excludedIds: [7, 9] });
        expect(toggleTagEntry(before, 7)).toEqual({ mode: 'all', query: 'saved query', excludedIds: [] });
        expect(before.excludedIds).toEqual([7]);
    });

    it.each(['selected', 'all'] as const)('enforces the %s id cap without poisoning old state or preventing removals', mode => {
        const ids = Array.from({ length: MAX_TAG_BATCH_IDS }, (_, index) => index + 1);
        const before: TagBatchSelection = mode === 'selected'
            ? { mode: 'selected', ids }
            : { mode: 'all', query: 'keep this query', excludedIds: ids };
        Object.freeze(ids);
        Object.freeze(before);
        expect(() => toggleTagEntry(before, MAX_TAG_BATCH_IDS + 1)).toThrow(RangeError);
        expect(ids.length).toBe(MAX_TAG_BATCH_IDS);
        expect(ids[ids.length - 1]).toBe(MAX_TAG_BATCH_IDS);
        const removed = toggleTagEntry(before, 1);
        const replaced = toggleTagEntry(removed, MAX_TAG_BATCH_IDS + 1);
        const replacedIds = replaced.mode === 'selected' ? replaced.ids : replaced.excludedIds;
        expect(replacedIds.length).toBe(MAX_TAG_BATCH_IDS);
        expect(replacedIds.includes(1)).toBe(false);
        expect(replacedIds.includes(MAX_TAG_BATCH_IDS + 1)).toBe(true);
        expect(ids[0]).toBe(1);
        expect(tagEntrySelected(before, MAX_TAG_BATCH_IDS + 1)).toBe(mode === 'all');
    });

    it.each([0, -0, -1, 1.5, NaN, Infinity, -Infinity, Number.MAX_SAFE_INTEGER + 1])('rejects invalid id %s in both modes without mutation', id => {
        const selections: TagBatchSelection[] = [
            { mode: 'selected', ids: [1] },
            { mode: 'all', query: 'unchanged', excludedIds: [2] },
        ];
        for (const selection of selections) {
            const before = JSON.stringify(selection);
            expect(() => toggleTagEntry(selection, id)).toThrow(RangeError);
            expect(() => tagEntrySelected(selection, id)).toThrow(RangeError);
            expect(JSON.stringify(selection)).toBe(before);
        }
    });

    it('accepts the largest safe id without changing its value', () => {
        const selected = toggleTagEntry(emptyTagSelection(), Number.MAX_SAFE_INTEGER);
        expect(selected).toEqual({ mode: 'selected', ids: [Number.MAX_SAFE_INTEGER] });
        expect(tagEntrySelected(selected, Number.MAX_SAFE_INTEGER)).toBe(true);
    });

    it('counts explicit ids independently of total and clamps excluded all-counts at zero', () => {
        expect(tagSelectionCount({ mode: 'selected', ids: [1, 2] }, 0)).toBe(2);
        expect(tagSelectionCount(selectAllTagEntries(''), 120000)).toBe(120000);
        const all: TagBatchSelection = { mode: 'all', query: 'filter', excludedIds: [1, 2] };
        expect(tagSelectionCount(all, 60)).toBe(58);
        expect(tagSelectionCount(all, 2)).toBe(0);
        expect(tagSelectionCount(all, 1)).toBe(0);
        expect(tagSelectionCount(all, 0)).toBe(0);
    });
});
