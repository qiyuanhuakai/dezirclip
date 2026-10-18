import { useEffect, useImperativeHandle, useMemo, useRef, useState, type KeyboardEvent, type Ref, type RefObject } from 'react';
import { Virtuoso, type VirtuosoHandle } from 'react-virtuoso';
import { Tag } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { getTagColor } from '../../../shared/lib/utils';

export interface TagFilterPickerHandle {
    handleInputKeyDown: (event: KeyboardEvent<HTMLInputElement>) => void;
}

interface TagFilterPickerProps {
    ref?: Ref<TagFilterPickerHandle>;
    id: string;
    tags: string[];
    colors: Record<string, string>;
    query: string;
    theme: string;
    inputRef: RefObject<HTMLInputElement | null>;
    t: (key: string) => string;
    onSelect: (tag: string) => void;
    onClose: () => void;
}

export default function TagFilterPicker({ ref, id, tags, colors, query, theme, inputRef, t, onSelect, onClose }: TagFilterPickerProps) {
    const list = useRef<VirtuosoHandle>(null);
    const [active, setActive] = useState(-1);
    const [catalog, setCatalog] = useState<string[] | null>(null);
    const [catalogError, setCatalogError] = useState(false);
    const [revision, setRevision] = useState(0);
    useEffect(() => {
        let disposed = false;
        void invoke<Record<string, number>>('get_all_tags_info').then(info => {
            if (!disposed) { setCatalog(Object.keys(info).sort((a, b) => a.localeCompare(b))); setCatalogError(false); }
        }).catch(() => { if (!disposed) setCatalogError(true); });
        return () => { disposed = true; };
    }, [revision]);
    const filtered = useMemo(() => {
        const needle = query.replace(/^tag:/i, '').trim().toLocaleLowerCase();
        return (catalog ?? tags).filter(tag => tag.toLocaleLowerCase().includes(needle));
    }, [catalog, tags, query]);
    useEffect(() => { setActive(-1); list.current?.scrollToIndex(0); }, [filtered]);
    useEffect(() => {
        const input = inputRef.current;
        if (active >= 0) input?.setAttribute('aria-activedescendant', `${id}-${active}`);
        else input?.removeAttribute('aria-activedescendant');
        return () => input?.removeAttribute('aria-activedescendant');
    }, [active, id, inputRef]);

    useImperativeHandle(ref, () => ({
        handleInputKeyDown(event) {
            if (event.nativeEvent.isComposing || event.ctrlKey || event.metaKey || event.altKey) return;
            if (event.key === 'Escape') {
                event.preventDefault(); event.stopPropagation(); onClose(); return;
            }
            if (event.key === 'Enter' && active >= 0 && filtered[active]) {
                event.preventDefault(); event.stopPropagation(); onSelect(filtered[active]); return;
            }
            if (!filtered.length) return;
            if (['ArrowDown', 'ArrowUp'].includes(event.key) || (active >= 0 && ['Home', 'End'].includes(event.key))) {
                event.preventDefault(); event.stopPropagation();
                const next = event.key === 'Home' ? 0 : event.key === 'End' ? filtered.length - 1
                    : active < 0 ? event.key === 'ArrowDown' ? 0 : filtered.length - 1
                        : Math.max(0, Math.min(filtered.length - 1, active + (event.key === 'ArrowDown' ? 1 : -1)));
                setActive(next);
                const target = document.getElementById(`${id}-${next}`);
                if (target) target.scrollIntoView({ block: 'nearest' });
                else list.current?.scrollToIndex({ index: next, align: 'start' });
            }
        },
    }), [active, filtered, id, onClose, onSelect]);

    return <div className="dc-tag-filter-picker" onBlur={event => {
        if (event.relatedTarget !== inputRef.current && !event.currentTarget.contains(event.relatedTarget)) onClose();
    }} onKeyDown={event => {
        if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); inputRef.current?.focus(); onClose(); }
    }}>
        <div className="dc-tag-filter-heading"><span><Tag size={13} aria-hidden="true" />{t('tags')}</span><small>{filtered.length}</small></div>
        {catalogError && <button type="button" className="dc-tag-filter-retry" onMouseDown={event => event.preventDefault()} onClick={() => { setRevision(value => value + 1); inputRef.current?.focus(); }}>{t('tag_load_failed')} · {t('common_retry')}</button>}
        {filtered.length ? <div className={`dc-tag-filter-size-${Math.min(filtered.length, 6)}`}>
            <Virtuoso ref={list} id={id} role="listbox" tabIndex={-1} aria-label={t('tags')} className="dc-tag-virtual-list" data={filtered} fixedItemHeight={36}
                computeItemKey={(_, tag) => tag} itemContent={(index, tag) => <div className="dc-tag-filter-row">
                    <button type="button" id={`${id}-${index}`} role="option" aria-selected={active === index} tabIndex={-1}
                        className={`dc-tag-filter-option${active === index ? ' dc-tag-filter-active' : ''}`} data-tag={tag}
                        onMouseDown={event => event.preventDefault()} onClick={() => onSelect(tag)}>
                        <span className="dc-tag-dot" style={{ backgroundColor: colors[tag] || getTagColor(tag, theme) }} />
                        <span>{tag}</span>
                    </button>
                </div>} />
        </div> : <div id={id} className="dc-tag-filter-empty" role="listbox" aria-label={t('tags')}>{t(catalog === null && !catalogError ? 'common_loading' : 'tag_no_matching_tags')}</div>}
    </div>;
}
