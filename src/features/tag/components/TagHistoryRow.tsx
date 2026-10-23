import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import ClipboardItem, { hideCompactPreviewGlobal } from '../../clipboard/components/ClipboardItem';
import type { ClipboardEntry, Locale } from '../../../shared/types';
import { beginPrivacyAccess, getPrivacyEntry, getPrivacyViewState, invokePrivacyBound, refreshPrivacyAfterConfirmation, subscribePrivacyInvalidation } from '../../../shared/lib/privacySession';
import { concealSensitiveEntry, isSensitiveEntry } from '../../../shared/lib/privacyRuntime';
import { clearRichTextSnapshots } from '../../../shared/lib/richTextSnapshot';
import { usePlatformCapabilities } from '../../../shared/hooks/usePlatformCapabilities';
import { ask } from '@tauri-apps/plugin-dialog';
import { capturePrivacyMetadataIntent, needsMetadataPersistenceConfirmation, privacyMetadataWriteArgs } from '../../../shared/lib/privacyMetadataIntent';
import type { PrivacyMetadataIntent } from '../../../shared/lib/privacyMetadataIntent';
import { matchesEntryIdentity } from '../../../shared/lib/historyIdentity';

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
    onEdit: (item: ClipboardEntry, revealedDetail?: ClipboardEntry) => void;
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
    const [detailState, setDetailState] = useState<{ source: ClipboardEntry; value: ClipboardEntry } | null>(null);
    const detail = detailState?.source === item ? detailState.value : null;
    const setDetail = (value: ClipboardEntry | null) => setDetailState(value ? { source: item, value } : null);
    const [editingTags, setEditingTags] = useState(false);
    const [tagInput, setTagInput] = useState('');
    const [tagBusy, setTagBusy] = useState(false);
    const fullItem = useRef<Promise<ClipboardEntry> | null>(null);
    const mounted = useRef(false);
    const detailGeneration = useRef(0);
    const currentItem = useRef(item);
    currentItem.current = item;
    const managing = useRef(managementMode);
    const sensitive = isSensitiveEntry(item);
    const capabilities = usePlatformCapabilities();
    managing.current = managementMode;

    const loadFullItem = useCallback(() => {
        if (!fullItem.current) {
            const generation = detailGeneration.current;
            const requestedItem = currentItem.current;
            const request: Promise<ClipboardEntry> = getPrivacyEntry(item.id, item.host_instance_id).then(value => {
                if (fullItem.current === request) fullItem.current = null;
                if (!mounted.current || managing.current || detailGeneration.current !== generation || currentItem.current !== requestedItem) throw new Error('PrivacyReadSuperseded');
                return value;
            }, error => {
                if (fullItem.current === request) fullItem.current = null;
                throw error;
            });
            fullItem.current = request;
        }
        return fullItem.current;
    }, [item.id, item.host_instance_id]);

    useEffect(() => {
        mounted.current = true;
        return () => { mounted.current = false; detailGeneration.current += 1; fullItem.current = null; };
    }, []);
    useEffect(() => subscribePrivacyInvalidation(() => {
        detailGeneration.current += 1;
        fullItem.current = null;
        setDetail(null);
        setTagInput('');
        setEditingTags(false);
    }), []);

    useEffect(() => {
        detailGeneration.current += 1;
        fullItem.current = null;
        setDetail(null);
        if (managementMode) { setEditingTags(false); setTagInput(''); }
    }, [managementMode]);

    useEffect(() => {
        let disposed = false;
        detailGeneration.current += 1;
        fullItem.current = null;
        setDetail(null);
        if (!sensitive && !compactMode && item.content_type === 'rich_text' && !item.html_content) {
            const access = beginPrivacyAccess();
            if (access) void invokePrivacyBound<ClipboardEntry>('get_history_resource_preview', { id: item.id, expectedHostInstanceId: item.host_instance_id, expectedEpoch: access.epoch, maxDimension: 480 })
                .then(next => { if (!disposed && access.isCurrent()) setDetail(next); }).catch(() => { if (!disposed) onError(); });
        }
        return () => { disposed = true; };
    }, [item, compactMode, loadFullItem, onError, sensitive]);

    const copy = (pasteWithFormat = false, pasteImageAsBase64 = false) => {
        if (managing.current) return;
        if (capabilities.pending || capabilities.failed
            || capabilities.report?.write.availability.status !== 'available') { onError(); return; }
        const access = beginPrivacyAccess();
        if (!access || (sensitive && !detail)) { onError(); return; }
        void invoke('copy_to_clipboard', { id: item.id, expectedHostInstanceId: item.host_instance_id, content: '', contentType: item.content_type,
            paste: !capabilities.report.native_wayland, deleteAfterUse: false, pasteWithFormat, pasteImageAsBase64,
            expectedPrivacyEpoch: sensitive ? access.epoch : undefined })
            .then(onChanged).catch(onError);
    };
    const authorizeMetadata = async (persists: boolean, original?: PrivacyMetadataIntent) => {
        const intent = original ?? capturePrivacyMetadataIntent(getPrivacyViewState());
        if (!intent) throw new Error('PrivacyStateUnavailable');
        let confirmed = false;
        if (needsMetadataPersistenceConfirmation(intent, item.id, persists)) {
            confirmed = await ask(t('privacy_metadata_persist_confirm'), { title: t('privacy_metadata_persist_title'), kind: 'warning' });
            await refreshPrivacyAfterConfirmation(confirmed ? intent.epoch : undefined);
            if (!confirmed) return null;
        }
        if (managing.current || !matchesEntryIdentity(currentItem.current, item.id, item.host_instance_id)) throw new Error('PrivacyMetadataSuperseded');
        return privacyMetadataWriteArgs(intent, getPrivacyViewState(), confirmed);
    };
    const changeTags = async (tags: string[], intent?: PrivacyMetadataIntent) => {
        if (tagBusy || managing.current) return;
        setTagBusy(true);
        try {
            const authorization = await authorizeMetadata(true, intent);
            if (!authorization) return;
            await invoke('update_tags', { id: item.id, expectedHostInstanceId: item.host_instance_id, tags, ...authorization });
            if (mounted.current) { setTagInput(''); setEditingTags(false); onChanged(); }
        } catch { if (mounted.current) onError(); }
        finally { if (mounted.current) setTagBusy(false); }
    };
    const rendered = detail ? { ...detail, tags: item.tags, is_pinned: item.is_pinned } : sensitive ? concealSensitiveEntry(item) : item;

    return <div className={`tag-history-row${managementMode ? ' tag-history-row--managing' : ''}`} data-entry-index={index} data-entry-id={item.id} tabIndex={0}
        aria-label={rendered.preview.slice(0, 100)}
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
            aria-label={`${t('tag_select_entry')}: ${rendered.preview.slice(0, 80)}`} onClick={event => event.stopPropagation()}
            onChange={() => onToggleSelection?.(item.id)}
            onKeyDown={event => {
                if (event.key === 'Enter') { event.preventDefault(); event.stopPropagation(); onToggleSelection?.(item.id); }
            }} />}
        <div className="tag-history-row-content" inert={managementMode}>
        <ClipboardItem key={managementMode ? 'selection' : 'content'} item={rendered} isSelected={managementMode ? selectedForDeletion : selected} windowPinned={isWindowPinned}
            isSensitiveHidden={sensitive && !detail} isRevealed={!!detail} showSensitiveVisibilityControl={sensitive} isEditingTags={editingTags}
            tagInput={tagInput} theme={theme} language={language} t={t} tagColors={tagColors}
            compactMode={compactMode} richTextSnapshotPreview={richTextSnapshotPreview} disableLayout
            onSelect={() => { if (!managing.current) onSelect(item.id); }} onCopy={copy}
            onToggleReveal={event => {
                event.stopPropagation();
                const generation = ++detailGeneration.current;
                fullItem.current = null;
                if (detail) { fullItem.current = null; clearRichTextSnapshots(); void hideCompactPreviewGlobal(); setDetail(null); return; }
                const access = beginPrivacyAccess();
                if (access) void loadFullItem().then(next => {
                    if (mounted.current && access.isCurrent() && currentItem.current === item && detailGeneration.current === generation && !managing.current) setDetail(next);
                }).catch(() => { if (mounted.current && access.isCurrent() && detailGeneration.current === generation && !managing.current) onError(); });
            }}
            onOpen={event => {
                event.stopPropagation(); const access = beginPrivacyAccess();
                if (!access || (sensitive && !detail)) { onError(); return; }
                if (!managing.current) void invoke('open_content', { id: item.id, expectedHostInstanceId: item.host_instance_id, content: '', contentType: item.content_type,
                    expectedPrivacyEpoch: sensitive ? access.epoch : undefined }).catch(onError);
            }}
            onTogglePin={async (event, intent) => {
                event.stopPropagation();
                if (managing.current) return;
                try {
                    const authorization = await authorizeMetadata(!item.is_pinned, intent);
                    if (!authorization) return;
                    await invoke('toggle_clipboard_pin', { id: item.id, expectedHostInstanceId: item.host_instance_id, isPinned: !item.is_pinned, ...authorization });
                    if (mounted.current) onChanged();
                } catch { if (mounted.current) onError(); }
            }}
            onDelete={event => { event.stopPropagation(); if (!managing.current) onDelete(item); }}
            onToggleTagEditor={event => { event.stopPropagation(); if (!tagBusy && !managing.current) setEditingTags(current => !current); }}
            onTagInput={setTagInput}
            onTagAdd={intent => {
                const next = tagInput.trim();
                if (next && !item.tags.includes(next)) return changeTags([...item.tags, next], intent);
            }}
            onTagDelete={(tag, intent) => changeTags(item.tags.filter(current => current !== tag), intent)}
            loadFullItem={loadFullItem} onEdit={() => {
                if (managing.current) return;
                if (!beginPrivacyAccess() || (sensitive && !detail)) { onError(); return; }
                onEdit(item, detail ?? undefined);
            }}
            onQRCode={() => {
                if (sensitive && !detail) { onError(); return; }
                const access = beginPrivacyAccess();
                const generation = detailGeneration.current;
                if (access && !managing.current) void loadFullItem().then(next => {
                    if (mounted.current && access.isCurrent() && !managing.current && currentItem.current === item && detailGeneration.current === generation) onQRCode(next);
                }).catch(() => { if (mounted.current && access.isCurrent() && detailGeneration.current === generation && !managing.current) onError(); });
            }}
            onTransformItemError={onError} />
        </div>
    </div>;
}
