import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import ClipboardItem from '../../clipboard/components/ClipboardItem';
import type { ClipboardEntry, Locale } from '../../../shared/types';

interface TagHistoryRowProps {
    item: ClipboardEntry;
    index: number;
    selected: boolean;
    compactMode: boolean;
    isWindowPinned: boolean;
    richTextSnapshotPreview: boolean;
    theme: string;
    language: Locale;
    t: (key: string) => string;
    tagColors: Record<string, string>;
    onSelect: (id: number) => void;
    onEdit: (item: ClipboardEntry) => void;
    onDelete: (item: ClipboardEntry) => void;
    onQRCode: (item: ClipboardEntry) => void;
    onError: () => void;
    onChanged: () => void;
    managementMode?: boolean;
    selectedForDeletion?: boolean;
    onToggleSelection?: (id: number) => void;
}

export function TagHistoryRow({ item, index, selected, compactMode, isWindowPinned,
    richTextSnapshotPreview, theme, language, t, tagColors, onSelect, onEdit, onDelete, onQRCode, onError, onChanged,
    managementMode = false, selectedForDeletion = false, onToggleSelection }: TagHistoryRowProps) {
    const [detail, setDetail] = useState<ClipboardEntry | null>(null);
    const [editingTags, setEditingTags] = useState(false);
    const [tagInput, setTagInput] = useState('');
    const [tagBusy, setTagBusy] = useState(false);
    const fullItem = useRef<Promise<ClipboardEntry> | null>(null);
    const mounted = useRef(false);
    const managing = useRef(managementMode);
    managing.current = managementMode;

    const loadFullItem = useCallback(() => {
        if (!fullItem.current) {
            const request: Promise<ClipboardEntry> = invoke<ClipboardEntry>('get_tag_item_detail', { id: item.id }).catch(error => {
                if (fullItem.current === request) fullItem.current = null;
                throw error;
            });
            fullItem.current = request;
        }
        return fullItem.current;
    }, [item.id]);

    useEffect(() => {
        mounted.current = true;
        return () => { mounted.current = false; fullItem.current = null; };
    }, []);

    useEffect(() => {
        if (managementMode) { setEditingTags(false); setTagInput(''); }
    }, [managementMode]);

    useEffect(() => {
        let disposed = false;
        fullItem.current = null;
        setDetail(null);
        if (!item.content && (['image', 'file', 'video'].includes(item.content_type)
            || (!compactMode && item.content_type === 'rich_text'))) {
            void loadFullItem().then(next => { if (!disposed) setDetail(next); }).catch(() => { if (!disposed) onError(); });
        }
        return () => { disposed = true; };
    }, [item, compactMode, loadFullItem, onError]);

    const copy = (pasteWithFormat = false, pasteImageAsBase64 = false) => {
        if (managing.current) return;
        void invoke('copy_to_clipboard', { id: item.id, content: '', contentType: item.content_type,
            paste: true, deleteAfterUse: false, pasteWithFormat, pasteImageAsBase64 })
            .then(onChanged).catch(onError);
    };
    const changeTags = async (tags: string[]) => {
        if (tagBusy || managing.current) return;
        setTagBusy(true);
        try {
            await invoke('update_tags', { id: item.id, tags });
            if (mounted.current) { setTagInput(''); setEditingTags(false); onChanged(); }
        } catch { if (mounted.current) onError(); }
        finally { if (mounted.current) setTagBusy(false); }
    };
    const rendered = detail ? { ...detail, tags: item.tags, is_pinned: item.is_pinned } : item;

    return <div className={`tag-history-row${managementMode ? ' tag-history-row--managing' : ''}`} data-entry-index={index} data-entry-id={item.id} tabIndex={0}
        aria-label={item.preview.slice(0, 100)}
        onClick={managementMode ? () => onToggleSelection?.(item.id) : undefined}
        onContextMenu={managementMode ? event => { event.preventDefault(); event.stopPropagation(); } : undefined}
        onKeyDown={event => {
            if (event.target !== event.currentTarget) return;
            if (managementMode) {
                if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); onToggleSelection?.(item.id); }
                return;
            }
            if (event.key === 'Enter') { event.preventDefault(); copy(); }
            if (event.key === 'Delete') { event.preventDefault(); onDelete(item); }
        }}>
        {managementMode && <input type="checkbox" className="tag-history-selection" checked={selectedForDeletion}
            aria-label={`${t('tag_select_entry')}: ${item.preview.slice(0, 80)}`} onClick={event => event.stopPropagation()}
            onChange={() => onToggleSelection?.(item.id)}
            onKeyDown={event => {
                if (event.key === 'Enter') { event.preventDefault(); event.stopPropagation(); onToggleSelection?.(item.id); }
            }} />}
        <div className="tag-history-row-content" inert={managementMode}>
        <ClipboardItem key={managementMode ? 'selection' : 'content'} item={rendered} isSelected={managementMode ? selectedForDeletion : selected} windowPinned={isWindowPinned}
            isSensitiveHidden={false} isRevealed={true} showSensitiveVisibilityControl={false} isEditingTags={editingTags}
            tagInput={tagInput} theme={theme} language={language} t={t} tagColors={tagColors}
            compactMode={compactMode} richTextSnapshotPreview={richTextSnapshotPreview} disableLayout
            onSelect={() => { if (!managing.current) onSelect(item.id); }} onCopy={copy}
            onToggleReveal={event => { event.stopPropagation(); }}
            onOpen={event => { event.stopPropagation(); if (!managing.current) void invoke('open_content', { id: item.id, content: '', contentType: item.content_type }).catch(onError); }}
            onTogglePin={event => { event.stopPropagation(); if (!managing.current) void invoke('toggle_clipboard_pin', { id: item.id, isPinned: !item.is_pinned }).then(onChanged).catch(onError); }}
            onDelete={event => { event.stopPropagation(); if (!managing.current) onDelete(item); }}
            onToggleTagEditor={event => { event.stopPropagation(); if (!tagBusy && !managing.current) setEditingTags(current => !current); }}
            onTagInput={setTagInput}
            onTagAdd={() => {
                const next = tagInput.trim();
                if (next && !item.tags.includes(next)) void changeTags([...item.tags, next]);
            }}
            onTagDelete={tag => { void changeTags(item.tags.filter(current => current !== tag)); }}
            loadFullItem={loadFullItem} onEdit={() => { if (!managing.current) onEdit(item); }}
            onQRCode={() => { if (!managing.current) void loadFullItem().then(next => { if (mounted.current && !managing.current) onQRCode(next); }).catch(onError); }}
            onTransformItemError={onError} />
        </div>
    </div>;
}
