import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ClipboardEntry } from '../../../shared/types';
import type { TagInfo, TagItemsPage, TagSort } from '../types';

const EMPTY_PAGE: TagItemsPage = { items: [], total: 0, has_more: false, next_offset: null };
const PAGE_SIZE = 60;

export function useTagLibrary() {
    const [tags, setTags] = useState<TagInfo[]>([]);
    const [colors, setColors] = useState<Record<string, string>>({});
    const [selectedTag, setSelectedTag] = useState<string | null>(null);
    const [query, setQueryState] = useState('');
    const [debouncedQuery, setDebouncedQuery] = useState('');
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
    const refreshTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
    const metadataTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
    const requestState = useRef({ selectedTag, debouncedQuery, sortBy, loading, error });
    requestState.current = { selectedTag, debouncedQuery, sortBy, loading, error };

    const publishPage = useCallback((next: TagItemsPage) => {
        pageRef.current = next;
        setPage(next);
    }, []);

    const invalidatePage = useCallback(() => {
        generation.current += 1;
        publishPage(EMPTY_PAGE);
        setError(false);
        setLoadingMore(false);
    }, [publishPage]);

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

    useEffect(() => {
        mounted.current = true;
        let disposed = false;
        const unlisteners: (() => void)[] = [];
        for (const event of ['clipboard-changed', 'clipboard-captured', 'clipboard-updated', 'clipboard-removed', 'tag-colors-updated']) {
            void listen(event, event === 'tag-colors-updated' ? refreshMetadata : refresh).then(unlisten => {
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
    }, [refresh, refreshMetadata]);

    useEffect(() => {
        const request = ++metadataGeneration.current;
        const epoch = selectionEpoch.current;
        setMetadataLoading(true);
        void Promise.all([
            invoke<Record<string, number>>('get_all_tags_info'),
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
    }, [revision, metadataRevision, selectTag]);

    useEffect(() => {
        const timer = setTimeout(() => setDebouncedQuery(query), 180);
        return () => clearTimeout(timer);
    }, [query]);

    useEffect(() => {
        const request = ++generation.current;
        publishPage(EMPTY_PAGE);
        setError(false);
        setLoadingMore(false);
        if (!selectedTag) {
            setLoading(false);
            return;
        }
        setLoading(true);
        if (query !== debouncedQuery) return;
        void invoke<TagItemsPage>('get_tag_items_page', {
            tag: selectedTag, query: debouncedQuery, sortBy, limit: PAGE_SIZE, offset: 0,
        }).then(next => {
            if (mounted.current && request === generation.current) publishPage(next);
        }).catch(() => {
            if (mounted.current && request === generation.current) setError(true);
        }).finally(() => {
            if (mounted.current && request === generation.current) setLoading(false);
        });
    }, [selectedTag, query, debouncedQuery, sortBy, revision, publishPage]);

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
        if (!active.selectedTag || active.loading || active.error || !current.has_more || current.next_offset === null) return;
        const request = generation.current;
        const key = `${request}:${current.next_offset}`;
        if (pending.current.has(key)) return;
        pending.current.add(key);
        setLoadingMore(true);
        void invoke<TagItemsPage>('get_tag_items_page', {
            tag: active.selectedTag, query: active.debouncedQuery, sortBy: active.sortBy, limit: PAGE_SIZE, offset: current.next_offset,
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
        tags, colors, selectedTag, selectTag, query, setQuery, sortBy, setSortBy, contentRevision: revision,
        items: page.items, total: page.total, hasMore: page.has_more,
        loading, loadingMore, metadataLoading, metadataError, error, loadMore, refresh, refreshMetadata,
    };
}
