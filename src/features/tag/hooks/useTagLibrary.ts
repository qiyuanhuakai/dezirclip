import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '../../../shared/lib/desktopInvoke';
import { listen } from '@tauri-apps/api/event';
import type { ClipboardEntry } from '../../../shared/types';
import type { TagInfo, TagItemsPage, TagSort } from '../types';
import { usePrivacySession } from '../../../shared/hooks/usePrivacySession';
import { useDebounce } from '../../../shared/hooks/useDebounce';
import { beginPrivacyAccess, invokePrivacyBound, subscribePrivacyInvalidation } from '../../../shared/lib/privacySession';
import { reuseUnchangedEntries } from '../../../shared/lib/reuseUnchanged';

const EMPTY_PAGE: TagItemsPage = { items: [], total: 0, has_more: false, next_offset: null };
const PAGE_SIZE = 60;

export function useTagLibrary() {
    const privacy = usePrivacySession();
    const [tags, setTags] = useState<TagInfo[]>([]);
    const [colors, setColors] = useState<Record<string, string>>({});
    const [selectedTag, setSelectedTag] = useState<string | null>(null);
    const [query, setQueryState] = useState('');
    const [isComposing, setIsComposing] = useState(false);
    const debouncedQuery = useDebounce(query, 300, !isComposing);
    const [sortBy, setSortState] = useState<TagSort>('time');
    const [page, setPage] = useState<TagItemsPage>(EMPTY_PAGE);
    const [loading, setLoading] = useState(false);
    const [loadingMore, setLoadingMore] = useState(false);
    const [metadataLoading, setMetadataLoading] = useState(true);
    const [error, setError] = useState(false);
    const [metadataError, setMetadataError] = useState(false);
    const [revision, setRevision] = useState(0);
    const [metadataRevision, setMetadataRevision] = useState(0);
    const mounted = useRef(false);
    const selected = useRef<string | null>(null);
    const selectionEpoch = useRef(0);
    const generation = useRef(0);
    const metadataGeneration = useRef(0);
    const pageRef = useRef(page);
    const pending = useRef(new Set<string>());
    const reconciling = useRef(false);
    const removedRows = useRef(new Set<number>());
    const refreshTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
    const metadataTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
    const requestState = useRef({ selectedTag, debouncedQuery, sortBy, loading, error });
    requestState.current = { selectedTag, debouncedQuery, sortBy, loading, error };

    const publishPage = useCallback((next: TagItemsPage) => {
        const reconciled = { ...next, items: reuseUnchangedEntries(pageRef.current.items, next.items) };
        pageRef.current = reconciled;
        setPage(reconciled);
    }, []);

    const invalidatePage = useCallback(() => {
        generation.current += 1;
        reconciling.current = false;
        removedRows.current.clear();
        publishPage(EMPTY_PAGE);
        setError(false);
        setLoadingMore(false);
    }, [publishPage]);
    useEffect(() => subscribePrivacyInvalidation((scope: 'all' | 'reveal' = 'all') => {
        if (scope === 'reveal') return;
        invalidatePage();
        metadataGeneration.current += 1;
        setTags([]);
        setRevision(value => value + 1);
    }), [invalidatePage]);

    const selectTag = useCallback((name: string | null) => {
        if (name === selected.current) return;
        selectionEpoch.current += 1;
        selected.current = name;
        invalidatePage();
        setSelectedTag(name);
    }, [invalidatePage]);

    const refresh = useCallback(() => {
        if (!mounted.current || refreshTimer.current !== null) return;
        refreshTimer.current = setTimeout(() => {
            refreshTimer.current = null;
            if (mounted.current) setRevision(value => value + 1);
        }, 120);
    }, []);

    const refreshMetadata = useCallback(() => {
        if (!mounted.current || metadataTimer.current !== null) return;
        metadataTimer.current = setTimeout(() => {
            metadataTimer.current = null;
            if (mounted.current) setMetadataRevision(value => value + 1);
        }, 120);
    }, []);

    const removeItems = useCallback((ids: ReadonlySet<number>, deletedCount?: number) => {
        const interrupted = reconciling.current;
        generation.current += 1;
        reconciling.current = false;
        const current = pageRef.current;
        const known = new Set(current.items.map(item => item.id));
        const unknown = [...ids].some(id => !known.has(id) && !removedRows.current.has(id));
        const items = current.items.filter(item => !ids.has(item.id));
        const removed = current.items.length - items.length;
        const alreadyRemoved = [...ids].filter(id => removedRows.current.has(id)).length;
        const total = Math.max(0, current.total - removed);
        for (const id of ids) if (known.has(id)) removedRows.current.add(id);
        publishPage({ ...current, items, total, has_more: current.has_more && total > items.length,
            next_offset: current.next_offset === null ? null : Math.max(0, current.next_offset - removed) });
        setLoading(false);
        setLoadingMore(false);
        refreshMetadata();
        if (interrupted || unknown || (deletedCount !== undefined && deletedCount > removed + alreadyRemoved)) refresh();
    }, [publishPage, refresh, refreshMetadata]);

    useEffect(() => {
        mounted.current = true;
        let disposed = false;
        const unlisteners: (() => void)[] = [];
        for (const event of ['clipboard-changed', 'clipboard-captured', 'clipboard-updated', 'tag-colors-updated']) {
            void listen<{ removedIds?: number[] } | undefined>(event, message => {
                if (event === 'tag-colors-updated') { refreshMetadata(); return; }
                const ids = message.payload?.removedIds;
                if (event === 'clipboard-changed' && Array.isArray(ids) && ids.length > 0 && ids.every(Number.isSafeInteger)) {
                    removeItems(new Set(ids));
                } else refresh();
            }).then(unlisten => {
                if (disposed) void Promise.resolve(unlisten()).catch(console.error);
                else unlisteners.push(unlisten);
            }).catch(console.error);
        }
        return () => {
            disposed = true;
            mounted.current = false;
            generation.current += 1;
            metadataGeneration.current += 1;
            if (refreshTimer.current !== null) clearTimeout(refreshTimer.current);
            if (metadataTimer.current !== null) clearTimeout(metadataTimer.current);
            refreshTimer.current = null;
            metadataTimer.current = null;
            unlisteners.forEach(unlisten => { void Promise.resolve(unlisten()).catch(console.error); });
        };
    }, [refresh, refreshMetadata, removeItems]);

    useEffect(() => {
        const request = ++metadataGeneration.current;
        if (privacy.phase !== 'ready') { setMetadataLoading(false); return; }
        const epoch = selectionEpoch.current;
        setMetadataLoading(true);
        void Promise.all([
            invokePrivacyBound<Record<string, number>>('get_all_tags_info'),
            invoke<Record<string, string>>('get_tag_colors'),
        ]).then(([tagMap, nextColors]) => {
            if (!mounted.current || request !== metadataGeneration.current) return;
            const nextTags = Object.entries(tagMap).map(([name, count]) => ({ name, count }))
                .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name));
            setTags(nextTags);
            setColors(nextColors ?? {});
            setMetadataError(false);
            if (epoch === selectionEpoch.current && !nextTags.some(tag => tag.name === selected.current)) {
                selectTag(nextTags[0]?.name ?? null);
            }
        }).catch(() => {
            if (mounted.current && request === metadataGeneration.current) setMetadataError(true);
        }).finally(() => {
            if (mounted.current && request === metadataGeneration.current) setMetadataLoading(false);
        });
    }, [revision, metadataRevision, selectTag, privacy.phase, privacy.snapshot?.privacy_epoch]);

    useEffect(() => {
        const request = ++generation.current;
        reconciling.current = false;
        setError(false);
        setLoadingMore(false);
        const access = beginPrivacyAccess("list");
        if (!selectedTag || privacy.phase !== 'ready' || !access) {
            publishPage(EMPTY_PAGE);
            setLoading(false);
            return;
        }
        const retainedCount = pageRef.current.items.length;
        setLoading(retainedCount === 0);
        if (isComposing || query !== debouncedQuery) return;
        reconciling.current = true;
        // Reconcile the loaded range without unmounting the virtual list. A
        // scope or privacy change still clears it synchronously above.
        void (async () => {
            let next: TagItemsPage = EMPTY_PAGE;
            const items: ClipboardEntry[] = [];
            let offset = 0;
            do {
                next = await invokePrivacyBound<TagItemsPage>('get_tag_items_page', {
                    tag: selectedTag, query: debouncedQuery, sortBy, limit: PAGE_SIZE, offset, expectedEpoch: access.epoch,
                });
                if (!mounted.current || request !== generation.current || !access.isCurrent()) return;
                items.push(...next.items);
                if (!next.has_more || next.next_offset === null || next.next_offset <= offset) break;
                offset = next.next_offset;
            } while (items.length < Math.max(PAGE_SIZE, retainedCount));
            return { ...next, items };
        })().then(next => {
            if (!next) return;
            if (mounted.current && request === generation.current) publishPage(next);
        }).catch(() => {
            if (mounted.current && request === generation.current) setError(true);
        }).finally(() => {
            if (mounted.current && request === generation.current) {
                reconciling.current = false;
                setLoading(false);
            }
        });
    }, [selectedTag, query, debouncedQuery, isComposing, sortBy, revision, publishPage, privacy.phase, privacy.snapshot?.privacy_epoch]);

    const setQuery = useCallback((value: string) => {
        invalidatePage();
        setQueryState(value);
    }, [invalidatePage]);

    const setSortBy = useCallback((value: TagSort) => {
        invalidatePage();
        setSortState(value);
    }, [invalidatePage]);

    const loadMore = useCallback(() => {
        const current = pageRef.current;
        const active = requestState.current;
        const access = beginPrivacyAccess("list");
        if (!access || reconciling.current || !active.selectedTag || active.loading || active.error || !current.has_more || current.next_offset === null) return;
        const request = generation.current;
        const key = `${request}:${current.next_offset}`;
        if (pending.current.has(key)) return;
        pending.current.add(key);
        setLoadingMore(true);
        void invokePrivacyBound<TagItemsPage>('get_tag_items_page', {
            tag: active.selectedTag, query: active.debouncedQuery, sortBy: active.sortBy, limit: PAGE_SIZE, offset: current.next_offset, expectedEpoch: access.epoch,
        }).then(next => {
            if (!mounted.current || request !== generation.current) return;
            const known = new Set(pageRef.current.items.map(item => item.id));
            const items: ClipboardEntry[] = [...pageRef.current.items, ...next.items.filter(item => !known.has(item.id))];
            publishPage({ ...next, items });
        }).catch(() => {
            if (mounted.current && request === generation.current) setError(true);
        }).finally(() => {
            pending.current.delete(key);
            if (mounted.current && request === generation.current) setLoadingMore(false);
        });
    }, [publishPage]);

    return {
        tags, colors, selectedTag, selectTag, query, setQuery, setIsComposing, sortBy, setSortBy, contentRevision: revision,
        items: page.items, total: page.total, hasMore: page.has_more,
        loading, loadingMore, metadataLoading, metadataError, error, loadMore, refresh, refreshMetadata, removeItems,
    };
}
