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
    privacyProtection: boolean;
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
}

export function TagHistoryRow({ item, index, selected, compactMode, isWindowPinned, privacyProtection,
    richTextSnapshotPreview, theme, language, t, tagColors, onSelect, onEdit, onDelete, onQRCode, onError, onChanged }: TagHistoryRowProps) {
    const [detail, setDetail] = useState<ClipboardEntry | null>(null);
    const [revealed, setRevealed] = useState(false);
    const [editingTags, setEditingTags] = useState(false);
    const [tagInput, setTagInput] = useState('');
    const [tagBusy, setTagBusy] = useState(false);
    const fullItem = useRef<Promise<ClipboardEntry> | null>(null);
    const mounted = useRef(false);
    const sensitive = item.tags.some(tag => ['sensitive', 'password', '密码'].includes(tag.toLowerCase()));
    const hidden = privacyProtection && sensitive && !revealed;

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
        let disposed = false;
        fullItem.current = null;
        setDetail(null);
        if (!hidden && !item.content && (['image', 'file', 'video'].includes(item.content_type)
            || (!compactMode && item.content_type === 'rich_text'))) {
            void loadFullItem().then(next => { if (!disposed) setDetail(next); }).catch(() => { if (!disposed) onError(); });
        }
        return () => { disposed = true; };
    }, [item, compactMode, hidden, loadFullItem, onError]);

    const copy = (pasteWithFormat = false, pasteImageAsBase64 = false) => {
        void invoke('copy_to_clipboard', { id: item.id, content: '', contentType: item.content_type,
            paste: true, deleteAfterUse: false, pasteWithFormat, pasteImageAsBase64 })
            .then(onChanged).catch(onError);
    };
    const changeTags = async (tags: string[]) => {
        if (tagBusy) return;
        setTagBusy(true);
        try {
            await invoke('update_tags', { id: item.id, tags });
            if (mounted.current) { setTagInput(''); setEditingTags(false); onChanged(); }
        } catch { if (mounted.current) onError(); }
        finally { if (mounted.current) setTagBusy(false); }
    };
    const rendered = detail ? { ...detail, tags: item.tags, is_pinned: item.is_pinned } : item;

    return <div className="tag-history-row" data-entry-index={index} data-entry-id={item.id} tabIndex={0}
        aria-label={hidden ? t('privacy_protection') : item.preview.slice(0, 100)}
        onKeyDown={event => {
            if (event.target !== event.currentTarget) return;
            if (event.key === 'Enter') { event.preventDefault(); copy(); }
            if (event.key === 'Delete') { event.preventDefault(); onDelete(item); }
        }}>
        <ClipboardItem item={rendered} isSelected={selected} windowPinned={isWindowPinned}
            isSensitiveHidden={hidden} isRevealed={revealed} isEditingTags={editingTags}
            tagInput={tagInput} theme={theme} language={language} t={t} tagColors={tagColors}
            compactMode={compactMode} richTextSnapshotPreview={richTextSnapshotPreview} disableLayout
            onSelect={() => onSelect(item.id)} onCopy={copy}
            onToggleReveal={event => { event.stopPropagation(); setRevealed(current => !current); }}
            onOpen={event => { event.stopPropagation(); void invoke('open_content', { id: item.id, content: '', contentType: item.content_type }).catch(onError); }}
            onTogglePin={event => { event.stopPropagation(); void invoke('toggle_clipboard_pin', { id: item.id, isPinned: !item.is_pinned }).then(onChanged).catch(onError); }}
            onDelete={event => { event.stopPropagation(); onDelete(item); }}
            onToggleTagEditor={event => { event.stopPropagation(); if (!tagBusy) setEditingTags(current => !current); }}
            onTagInput={setTagInput}
            onTagAdd={() => {
                const next = tagInput.trim();
                if (next && !item.tags.includes(next)) void changeTags([...item.tags, next]);
            }}
            onTagDelete={tag => { void changeTags(item.tags.filter(current => current !== tag)); }}
            loadFullItem={loadFullItem} onEdit={() => onEdit(item)}
            onQRCode={() => { void loadFullItem().then(next => { if (mounted.current) onQRCode(next); }).catch(onError); }}
            onTransformItemError={onError} />
    </div>;
}
