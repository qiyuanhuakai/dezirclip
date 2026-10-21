import { lazy, Suspense, useCallback, useEffect, useId, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { Virtuoso, type VirtuosoHandle } from 'react-virtuoso';
import { ArrowDownWideNarrow, Check, ChevronDown, ChevronLeft, ChevronRight, Edit2, List, ListChecks, ListCollapse, LoaderCircle, MoreHorizontal, Plus, Search, Tag, Trash2, X } from 'lucide-react';
import type { ClipboardEntry, Locale } from '../../../shared/types';
import { errorMessage } from '../../../shared/lib/errorMessage';
import { beginPrivacyAccess, invokePrivacyBound, subscribePrivacyInvalidation } from '../../../shared/lib/privacySession';
import { isSensitiveEntry } from '../../../shared/lib/privacyRuntime';
import { usePrivacySession } from '../../../shared/hooks/usePrivacySession';
import { useTagLibrary } from '../hooks/useTagLibrary';
import { TagDialog } from './TagDialog';
import { TagHistoryRow } from './TagHistoryRow';
import { useSettingsLayout } from '../../settings/hooks/useSettingsLayout';
import { emptyTagSelection, MAX_TAG_BATCH_IDS, selectAllTagEntries, tagEntrySelected, tagSelectionCount, toggleTagEntry, type TagBatchSelection } from '../utils/tagBatchSelection';

const QrCodeDialog = lazy(() => import('../../clipboard/components/QrCodeDialog'));

interface TagManagerProps {
    t: (key: string) => string;
    theme: string;
    language?: Locale;
    compactMode?: boolean;
    isWindowPinned?: boolean;
    privacyProtection?: boolean;
    tagColors?: Record<string, string>;
    richTextSnapshotPreview?: boolean;
    onCompactModeChange?: (compact: boolean) => void;
}
type Dialog = { kind: 'createTag' }
    | { kind: 'renameTag'; tag: string }
    | { kind: 'removeTag'; tag: string }
    | { kind: 'createItem'; tag: string; expectedPrivacyEpoch?: number }
    | { kind: 'editItem'; id: number; hostInstanceId?: string; protectedContent: boolean; expectedPrivacyEpoch: number }
    | { kind: 'deleteSelection'; tag: string; selection: TagBatchSelection; count: number; scope: string }
    | { kind: 'deleteItem'; item: ClipboardEntry };

function protectedTag(name: string): boolean { return isSensitiveEntry({ tags: [name] }); }

export default function TagManager({ t, theme, language = 'zh', compactMode = false, isWindowPinned = false,
    privacyProtection = true, tagColors, richTextSnapshotPreview = false, onCompactModeChange }: TagManagerProps) {
    const library = useTagLibrary();
    const privacy = usePrivacySession();
    const privacyActive = privacy.phase === 'ready' && privacy.snapshot?.session.active === true;
    const { selectedTag, selectTag, refresh } = library;
    const [tagSearch, setTagSearch] = useState('');
    const { split } = useSettingsLayout();
    const [collapsed, setCollapsed] = useState(false);
    const [pickerOpen, setPickerOpen] = useState(false);
    const [pickerActiveTag, setPickerActiveTag] = useState<string | null>(null);
    const [managementMode, setManagementMode] = useState(false);
    const selectionContext = JSON.stringify([selectedTag, library.query]);
    const selectionScope = JSON.stringify([selectedTag, library.query, library.contentRevision, library.total]);
    const [entrySelection, setEntrySelection] = useState(() => ({ context: selectionContext, scope: selectionScope, selection: emptyTagSelection() as TagBatchSelection }));
    const selection = entrySelection.scope === selectionScope ? entrySelection.selection : emptyTagSelection();
    const selectedCount = tagSelectionCount(selection, library.total);
    const [batchFeedback, setBatchFeedback] = useState<{ message: string; error: boolean } | null>(null);
    const [menuOpen, setMenuOpen] = useState(false);
    const [selectedId, setSelectedId] = useState<number | null>(null);
    const [detailRevision, setDetailRevision] = useState(0);
    const [dialog, setDialog] = useState<Dialog | null>(null);
    const [qrEntry, setQrEntry] = useState<ClipboardEntry | null>(null);
    const [value, setValue] = useState('');
    const [busy, setBusy] = useState(false);
    const [dialogError, setDialogError] = useState<string | null>(null);
    const [confirmPersistence, setConfirmPersistence] = useState(false);
    const [actionError, setActionError] = useState(false);
    const root = useRef<HTMLDivElement>(null);
    const menu = useRef<HTMLDivElement>(null);
    const menuButton = useRef<HTMLButtonElement>(null);
    const manageButton = useRef<HTMLButtonElement>(null);
    const picker = useRef<HTMLDivElement>(null);
    const pickerButton = useRef<HTMLButtonElement>(null);
    const pickerId = useId();
    const tagListId = useId();
    const searchInput = useRef<HTMLInputElement>(null);
    const tagSearchInput = useRef<HTMLInputElement>(null);
    const tagList = useRef<VirtuosoHandle>(null);
    const list = useRef<VirtuosoHandle>(null);
    const detailSequence = useRef(0);
    const focusSequence = useRef(0);
    const mounted = useRef(true);
    const colors = tagColors ?? library.colors;
    const filteredTags = useMemo(() => {
        const filter = tagSearch.trim().toLocaleLowerCase();
        return library.tags.filter(tag => tag.name.toLocaleLowerCase().includes(filter));
    }, [library.tags, tagSearch]);
    const notifyError = useCallback(() => { if (mounted.current) setActionError(true); }, []);
    const changed = useCallback(() => { setDetailRevision(current => current + 1); refresh(); }, [refresh]);
    useEffect(() => subscribePrivacyInvalidation(() => {
        detailSequence.current += 1;
        setQrEntry(null);
        setDialog(null);
        setValue('');
        setConfirmPersistence(false);
        setMenuOpen(false);
    }), []);

    useEffect(() => {
        mounted.current = true;
        return () => { mounted.current = false; detailSequence.current += 1; focusSequence.current += 1; };
    }, []);
    useEffect(() => { detailSequence.current += 1; setActionError(false); setSelectedId(null); setMenuOpen(false); }, [selectedTag, library.query, library.sortBy]);
    useEffect(() => {
        const hadSelection = entrySelection.selection.mode === 'all' || entrySelection.selection.ids.length > 0;
        if (entrySelection.context !== selectionContext) setBatchFeedback(null);
        else if (entrySelection.scope !== selectionScope && hadSelection) {
            setBatchFeedback({ message: t('tag_selection_changed'), error: false });
        }
        setEntrySelection({ context: selectionContext, scope: selectionScope, selection: emptyTagSelection() });
        setDialog(current => current?.kind === 'deleteSelection' && current.scope !== selectionScope && !busy ? null : current);
    }, [selectionScope]);
    useEffect(() => {
        if (!actionError) return;
        const timer = setTimeout(() => setActionError(false), 4000);
        return () => clearTimeout(timer);
    }, [actionError]);
    useEffect(() => {
        if (!menuOpen) return;
        const dismiss = (event: PointerEvent) => {
            if (event.target instanceof Node && !menu.current?.contains(event.target) && !menuButton.current?.contains(event.target)) setMenuOpen(false);
        };
        document.addEventListener('pointerdown', dismiss);
        menu.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
        return () => document.removeEventListener('pointerdown', dismiss);
    }, [menuOpen]);
    useEffect(() => { setPickerOpen(false); }, [split]);
    useEffect(() => {
        if (!pickerOpen) return;
        const dismiss = (event: PointerEvent) => {
            if (event.target instanceof Node && !picker.current?.contains(event.target) && !pickerButton.current?.contains(event.target)) setPickerOpen(false);
        };
        document.addEventListener('pointerdown', dismiss);
        tagSearchInput.current?.focus();
        return () => { document.removeEventListener('pointerdown', dismiss); focusSequence.current += 1; };
    }, [pickerOpen]);

    const clearSelection = () => setEntrySelection({ context: selectionContext, scope: selectionScope, selection: emptyTagSelection() });
    const selectAll = () => {
        if (!selectedTag || library.loading || library.error || library.total === 0) return;
        setEntrySelection({ context: selectionContext, scope: selectionScope, selection: selection.mode === 'all' && selection.excludedIds.length === 0
            ? emptyTagSelection() : selectAllTagEntries(library.query) });
        setBatchFeedback(null);
    };
    const toggleEntry = (id: number) => {
        try {
            setEntrySelection({ context: selectionContext, scope: selectionScope, selection: toggleTagEntry(selection, id) });
            setBatchFeedback(null);
        } catch {
            setBatchFeedback({ message: t('tag_selection_limit').replace('{limit}', String(MAX_TAG_BATCH_IDS)), error: true });
        }
    };
    const toggleManagement = () => {
        detailSequence.current += 1;
        setManagementMode(current => !current); clearSelection(); setBatchFeedback(null); setMenuOpen(false); setPickerOpen(false);
    };

    const openDialog = useCallback((next: Dialog) => {
        const access = beginPrivacyAccess();
        if (next.kind === 'createItem' && !access) { notifyError(); return; }
        detailSequence.current += 1;
        setMenuOpen(false); setPickerOpen(false); setDialogError(null);
        setValue(next.kind === 'renameTag' ? next.tag : next.kind === 'createTag' ? tagSearch.trim() : '');
        setConfirmPersistence(false);
        setDialog(next.kind === 'createItem' ? { ...next, expectedPrivacyEpoch: access?.epoch } : next);
    }, [tagSearch, notifyError]);
    const editItem = useCallback((item: ClipboardEntry, revealedDetail?: ClipboardEntry) => {
        const request = ++detailSequence.current;
        const access = beginPrivacyAccess();
        if (!access) { notifyError(); return; }
        setConfirmPersistence(false);
        if (isSensitiveEntry(item)) {
            if (!revealedDetail || revealedDetail.id !== item.id
                || revealedDetail.host_instance_id !== item.host_instance_id) { notifyError(); return; }
            setDialogError(null); setValue(revealedDetail.content);
            setDialog({ kind: 'editItem', id: item.id, hostInstanceId: item.host_instance_id, protectedContent: true, expectedPrivacyEpoch: access.epoch });
            return;
        }
        void invokePrivacyBound<string>('get_clipboard_content', { id: item.id, expectedHostInstanceId: item.host_instance_id }).then(content => {
            if (!mounted.current || request !== detailSequence.current) return;
            setDialogError(null); setValue(content); setDialog({ kind: 'editItem', id: item.id, hostInstanceId: item.host_instance_id, protectedContent: false, expectedPrivacyEpoch: access.epoch });
        }).catch(() => { if (request === detailSequence.current) notifyError(); });
    }, [notifyError]);
    const deleteItem = useCallback((item: ClipboardEntry) => openDialog({ kind: 'deleteItem', item }), [openDialog]);
    const closeDialog = () => {
        if (busy) return;
        if (dialog?.kind === 'createTag' && !split) requestAnimationFrame(() => pickerButton.current?.focus());
        setDialog(null);
    };

    const focusVirtualItem = (selector: string, scroll: () => void) => {
        const request = ++focusSequence.current;
        const focus = (target: HTMLElement) => { target.scrollIntoView({ block: 'nearest' }); target.focus({ preventScroll: true }); };
        const existing = root.current?.querySelector<HTMLElement>(selector);
        if (existing) { focus(existing); return; }
        const attempt = (remaining: number) => requestAnimationFrame(() => {
            if (!mounted.current || request !== focusSequence.current) return;
            const target = root.current?.querySelector<HTMLElement>(selector);
            if (target) focus(target);
            else if (remaining > 0) { scroll(); attempt(remaining - 1); }
        });
        scroll(); attempt(30);
    };

    const saveDialog = async () => {
        if (!dialog || busy) return;
        const current = dialog;
        if (current.kind === 'deleteSelection' && current.scope !== selectionScope) {
            setDialog(null); clearSelection();
            setBatchFeedback({ message: t('tag_selection_changed'), error: false });
            return;
        }
        const trimmed = value.trim();
        if (['createTag', 'renameTag', 'createItem', 'editItem'].includes(current.kind) && !trimmed) return;
        if (current.kind === 'renameTag' && protectedTag(trimmed)) { setDialogError(t('tag_sensitive_target')); return; }
        if (current.kind === 'removeTag' && protectedTag(current.tag) && privacyProtection) { setDialogError(t('tag_sensitive_delete_locked')); return; }
        if ((current.kind === 'createItem' || current.kind === 'editItem') && (privacy.phase !== 'ready'
            || current.expectedPrivacyEpoch !== beginPrivacyAccess()?.epoch
            || (privacyActive && !confirmPersistence))) { setDialogError(t('privacy_action_failed')); return; }
        setBusy(true); setDialogError(null);
        try {
            let deletedCount: number | null = null;
            if (current.kind === 'createTag') {
                const existing = library.tags.find(tag => tag.name.toLocaleLowerCase() === trimmed.toLocaleLowerCase());
                if (!existing) await invoke('create_new_tag', { tagName: trimmed });
                selectTag(existing?.name ?? trimmed); setTagSearch('');
            } else if (current.kind === 'renameTag') {
                await invoke('rename_tag_globally', { oldName: current.tag, newName: trimmed });
                if (selectedTag === current.tag) selectTag(trimmed);
            } else if (current.kind === 'removeTag') {
                if (protectedTag(current.tag)) await invoke('delete_sensitive_tag_with_contents', { tagName: current.tag });
                else await invoke('remove_tag_from_all', { tagName: current.tag });
                if (selectedTag === current.tag) selectTag(null);
            } else if (current.kind === 'createItem') {
                await invoke('add_manual_item', { content: value, contentType: 'text', tags: [current.tag],
                    expectedPrivacyEpoch: current.expectedPrivacyEpoch, confirmPersistence: privacyActive && confirmPersistence });
            } else if (current.kind === 'editItem') {
                await invoke('update_item_content', { id: current.id, expectedHostInstanceId: current.hostInstanceId, newContent: value,
                    expectedPrivacyEpoch: current.expectedPrivacyEpoch, confirmPersistence: privacyActive && confirmPersistence,
                    allowSensitive: current.protectedContent });
            } else if (current.kind === 'deleteSelection') {
                const result = await invoke<{ deletedCount: number }>('delete_tag_entries', { tagName: current.tag, selection: current.selection });
                deletedCount = result.deletedCount;
            } else await invoke('delete_clipboard_entry', { id: current.item.id, expectedHostInstanceId: current.item.host_instance_id });
            if (!mounted.current) return;
            setDialog(null); changed();
            if (deletedCount !== null) {
                clearSelection(); setBatchFeedback({ message: t('tag_batch_deleted').replace('{count}', String(deletedCount)), error: false });
            }
            if (current.kind === 'createTag' && !split) requestAnimationFrame(() => pickerButton.current?.focus());
            void emit('clipboard-changed').catch(console.error);
        } catch (reason) {
            if (mounted.current) {
                if (current.kind === 'deleteSelection') {
                    setDialog(null); clearSelection(); changed();
                    setBatchFeedback({ message: errorMessage(reason) || t('tag_action_failed'), error: true });
                } else setDialogError(current.kind === 'removeTag' && protectedTag(current.tag)
                    ? errorMessage(reason) || t('tag_action_failed') : t('tag_action_failed'));
            }
        }
        finally { if (mounted.current) setBusy(false); }
    };

    const handleKeyboard = (event: KeyboardEvent<HTMLDivElement>) => {
        if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) return;
        const target = event.target as HTMLElement;
        if (target.closest('dialog')) return;
        if (pickerOpen && event.key === 'Escape') {
            event.preventDefault(); event.stopPropagation(); setPickerOpen(false); pickerButton.current?.focus(); return;
        }
        if (pickerOpen && target === tagSearchInput.current && ['ArrowDown', 'ArrowUp'].includes(event.key) && filteredTags.length) {
            const next = event.key === 'ArrowDown' ? 0 : filteredTags.length - 1;
            event.preventDefault();
            focusVirtualItem(`[data-tag-index="${next}"]`, () => tagList.current?.scrollToIndex({ index: next, align: 'start' }));
            return;
        }
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'f') {
            event.preventDefault(); searchInput.current?.focus(); return;
        }
        const editable = target.closest('textarea, input:not([type="checkbox"])');
        if (managementMode && !editable) {
            if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') { event.preventDefault(); selectAll(); return; }
            if (event.key === 'Escape') { event.preventDefault(); toggleManagement(); manageButton.current?.focus(); return; }
            if (event.key === 'Delete' && selectedTag && selectedCount > 0 && !busy) {
                event.preventDefault(); openDialog({ kind: 'deleteSelection', tag: selectedTag, selection, count: selectedCount, scope: selectionScope }); return;
            }
        }
        if (target.closest('input, textarea, [role="menu"]')) return;
        if (event.key === 'F2' && !managementMode && selectedTag && !protectedTag(selectedTag)) {
            event.preventDefault(); openDialog({ kind: 'renameTag', tag: selectedTag }); return;
        }
        const tagIndex = target.dataset.tagIndex;
        if (tagIndex !== undefined && ['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
            const index = Number(tagIndex);
            const next = Math.max(0, Math.min(filteredTags.length - 1, event.key === 'Home' ? 0 : event.key === 'End' ? filteredTags.length - 1 : index + (event.key === 'ArrowDown' ? 1 : -1)));
            event.preventDefault();
            focusVirtualItem(`[data-tag-index="${next}"]`, () => tagList.current?.scrollToIndex({ index: next, align: 'start' }));
        }
        const itemIndex = target.dataset.entryIndex;
        if (itemIndex !== undefined && ['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
            const next = Math.max(0, Math.min(library.items.length - 1, event.key === 'Home' ? 0 : event.key === 'End' ? library.items.length - 1 : Number(itemIndex) + (event.key === 'ArrowDown' ? 1 : -1)));
            event.preventDefault();
            focusVirtualItem(`[data-entry-index="${next}"]`, () => list.current?.scrollToIndex({ index: next, align: 'center' }));
        }
    };

    const isTextDialog = dialog?.kind === 'createItem' || dialog?.kind === 'editItem';
    const protectedDialog = dialog?.kind === 'editItem' ? dialog.protectedContent
        : dialog?.kind === 'createItem' && protectedTag(dialog.tag);
    const isNameDialog = dialog?.kind === 'createTag' || dialog?.kind === 'renameTag';
    const destructiveTag = dialog?.kind === 'removeTag' && protectedTag(dialog.tag);
    const dialogTitle = dialog?.kind === 'deleteSelection' ? t('tag_batch_delete_title') : dialog?.kind === 'createTag' ? t('tag_create') : dialog?.kind === 'renameTag' ? t('tag_rename')
        : dialog?.kind === 'removeTag' ? destructiveTag ? t('tag_sensitive_delete_title') : t('tag_remove_title').replace('{tag}', dialog.tag)
            : dialog?.kind === 'createItem' ? t('add_item') : dialog?.kind === 'editItem' ? t('edit_item') : t('confirm_delete');
    const menuItem = (icon: ReactNode, label: string, action: () => void, disabled = false) => <button type="button"
        className="item-context-menu__item" role="menuitem" disabled={disabled}
        onClick={() => { setMenuOpen(false); action(); }}><span className="item-context-menu__item-label">{icon}{label}</span></button>;
    const renderTagList = (dropdown: boolean) => <Virtuoso ref={tagList} id={tagListId} data={filteredTags} fixedItemHeight={38}
        role={dropdown ? 'listbox' : undefined} aria-label={t('tags')} computeItemKey={(_, tag) => tag.name}
        itemContent={(index, tag) => <div className="tag-manager-tag-cell"><button type="button" data-tag-index={index}
            className={dropdown ? `item-context-menu__item tag-manager-tag${pickerActiveTag === tag.name ? ' item-context-menu__item--active' : ''}`
                : `settings-rail-item tag-manager-tag${selectedTag === tag.name ? ' is-active' : ''}`}
            role={dropdown ? 'option' : undefined} aria-selected={dropdown ? selectedTag === tag.name : undefined}
            onMouseEnter={dropdown ? () => setPickerActiveTag(tag.name) : undefined} onFocus={dropdown ? () => setPickerActiveTag(tag.name) : undefined}
            onClick={() => {
                selectTag(tag.name);
                if (dropdown) { setPickerOpen(false); pickerButton.current?.focus(); }
            }} aria-label={`${tag.name} · ${tag.count}`} aria-current={!dropdown && selectedTag === tag.name ? 'page' : undefined} title={`${tag.name} · ${tag.count}`}>
            {collapsed && !dropdown ? <span className="tag-manager-tag-initial" aria-hidden="true">{Array.from(tag.name)[0]}</span>
                : <><span className={`tag-manager-tag-name${dropdown ? ' item-context-menu__item-label' : ''}`}>{tag.name}</span><small>{tag.count}</small></>}
        </button></div>} />;

    return <div ref={root} className={`tag-manager${split && collapsed ? ' tag-manager--collapsed' : ''}`} onKeyDown={handleKeyboard}
        onPointerDownCapture={() => { void invoke('activate_window_focus').catch(console.error); }}>
        {split && <aside className="settings-rail tag-manager-rail" aria-label={t('tags')}>
            <div className="tag-manager-rail-tools">
                <button type="button" className="btn-icon" onClick={() => setCollapsed(current => !current)}
                    title={t(collapsed ? 'tag_expand_navigation' : 'tag_collapse_navigation')} aria-label={t(collapsed ? 'tag_expand_navigation' : 'tag_collapse_navigation')}>
                    {collapsed ? <ChevronRight size={16} /> : <ChevronLeft size={16} />}
                </button>
                {!collapsed && <button type="button" className="btn-icon" title={t('tag_create')} aria-label={t('tag_create')} onClick={() => openDialog({ kind: 'createTag' })}><Plus size={16} /></button>}
            </div>
            {collapsed ? <button type="button" className="btn-icon tag-manager-rail-search-toggle" title={t('tag_search_label')} aria-label={t('tag_search_label')}
                onClick={() => { setCollapsed(false); requestAnimationFrame(() => tagSearchInput.current?.focus()); }}><Search size={15} /></button>
                : <div className="search-container tag-manager-rail-search"><Search className="search-icon" size={13} /><input ref={tagSearchInput} className="search-input"
                    value={tagSearch} onChange={event => setTagSearch(event.target.value)} placeholder={t('tag_search_label')} aria-label={t('tag_search_label')} /></div>}
            <div className="tag-manager-tag-list" aria-busy={library.metadataLoading}>
                {renderTagList(false)}
            </div>
            {collapsed && <button type="button" className="btn-icon tag-manager-rail-add" title={t('tag_create')} aria-label={t('tag_create')} onClick={() => openDialog({ kind: 'createTag' })}><Plus size={16} /></button>}
        </aside>}
        <section className="tag-manager-content" aria-label={selectedTag ?? t('tag_manager')}>
            <div className="tag-manager-toolbar">
                {split ? <span className="tag-manager-current" title={selectedTag ?? ''}><Tag size={14} /><span>{selectedTag ?? t('tags')}</span></span>
                    : <button ref={pickerButton} type="button" className={`btn-icon tag-manager-current tag-manager-picker-trigger${pickerOpen ? ' active' : ''}`}
                        aria-label={`${t('tags')}: ${selectedTag ?? t('tags')}`} title={selectedTag ?? t('tags')}
                        aria-haspopup="dialog" aria-expanded={pickerOpen} aria-controls={pickerOpen ? pickerId : undefined}
                        onClick={() => { setMenuOpen(false); setPickerActiveTag(selectedTag); setPickerOpen(current => !current); }}
                        onKeyDown={event => {
                            if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) return;
                            if (['ArrowDown', 'ArrowUp'].includes(event.key)) { event.preventDefault(); setMenuOpen(false); setPickerActiveTag(selectedTag); setPickerOpen(true); }
                        }}><Tag size={14} /><span>{selectedTag ?? t('tags')}</span><ChevronDown size={13} /></button>}
                {!split && pickerOpen && <div ref={picker} id={pickerId} role="dialog" aria-label={t('tag_search_label')}
                    className="item-context-menu tag-manager-picker" onBlur={event => {
                        if (event.relatedTarget instanceof Node && !event.currentTarget.contains(event.relatedTarget) && event.relatedTarget !== pickerButton.current) setPickerOpen(false);
                    }}>
                    <div className="tag-manager-picker-tools">
                        <div className="search-container tag-manager-picker-search"><Search className="search-icon" size={13} />
                            <input ref={tagSearchInput} className="search-input" value={tagSearch} onChange={event => { focusSequence.current += 1; setTagSearch(event.target.value); }}
                                placeholder={t('tag_search_label')} aria-label={t('tag_search_label')} role="combobox" aria-autocomplete="list" aria-expanded="true" aria-controls={tagListId} />
                        </div>
                        <button type="button" className="btn-icon" title={t('tag_create')} aria-label={t('tag_create')} onClick={() => openDialog({ kind: 'createTag' })}><Plus size={16} /></button>
                    </div>
                    {filteredTags.length ? <div className={`tag-manager-tag-list tag-manager-picker-size-${Math.min(filteredTags.length, 6)}`} aria-busy={library.metadataLoading}>{renderTagList(true)}</div>
                        : <div id={tagListId} className="dc-tag-filter-empty" role="listbox" aria-label={t('tags')}><span role="status">{t(library.metadataLoading ? 'common_loading' : library.metadataError ? 'tag_load_failed' : 'tag_no_matching_tags')}</span></div>}
                </div>}
                <button type="button" className={`btn-icon${compactMode ? ' active' : ''}`} onClick={() => onCompactModeChange?.(!compactMode)}
                    disabled={!onCompactModeChange} title={t(compactMode ? 'tag_mode_detailed' : 'tag_mode_simple')}
                    aria-label={t(compactMode ? 'tag_mode_detailed' : 'tag_mode_simple')} aria-pressed={compactMode}>
                    {compactMode ? <List size={16} /> : <ListCollapse size={16} />}
                </button>
                <button ref={manageButton} type="button" className={`btn-icon${managementMode ? ' active' : ''}`}
                    disabled={busy || (!managementMode && !selectedTag)} aria-pressed={managementMode} onClick={toggleManagement}
                    title={t(managementMode ? 'tag_manage_done' : 'tag_manage')} aria-label={t(managementMode ? 'tag_manage_done' : 'tag_manage')}>
                    {managementMode ? <Check size={16} /> : <ListChecks size={16} />}
                </button>
                <button type="button" className="btn-icon" disabled={!selectedTag || managementMode || busy} title={t('add_item')} aria-label={t('add_item')}
                    onClick={() => { if (selectedTag) openDialog({ kind: 'createItem', tag: selectedTag }); }}><Plus size={16} /></button>
                <button ref={menuButton} type="button" className={`btn-icon${menuOpen ? ' active' : ''}`} title={t('tag_actions')} aria-label={t('tag_actions')}
                    disabled={!selectedTag || managementMode || busy} aria-haspopup="menu" aria-expanded={menuOpen} onClick={() => setMenuOpen(current => !current)}><MoreHorizontal size={17} /></button>
                {menuOpen && selectedTag && <div ref={menu} className="item-context-menu tag-manager-menu" role="menu" onKeyDown={event => {
                    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); setMenuOpen(false); menuButton.current?.focus(); }
                    if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
                        event.preventDefault();
                        const options = [...event.currentTarget.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')];
                        const index = options.indexOf(document.activeElement as HTMLButtonElement);
                        const next = event.key === 'Home' ? 0 : event.key === 'End' ? options.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + options.length) % options.length;
                        options[next]?.focus();
                    }
                }}>
                    {menuItem(<ArrowDownWideNarrow size={14} />, t(library.sortBy === 'time' ? 'sort_usage' : 'sort_time'), () => library.setSortBy(library.sortBy === 'time' ? 'count' : 'time'))}
                    {menuItem(<Edit2 size={14} />, t('tag_rename'), () => openDialog({ kind: 'renameTag', tag: selectedTag }), protectedTag(selectedTag))}
                    <div className="item-context-menu__separator" />
                    {menuItem(<Trash2 size={14} />, t(protectedTag(selectedTag) ? 'common_delete' : 'tag_remove'), () => openDialog({ kind: 'removeTag', tag: selectedTag }), protectedTag(selectedTag) && privacyProtection)}
                </div>}
            </div>
            <div className="search-container tag-manager-search"><Search className="search-icon" size={14} />
                <input ref={searchInput} className="search-input" value={library.query} maxLength={256} disabled={!selectedTag}
                    onChange={event => library.setQuery(event.target.value)} placeholder={t('tag_content_search')} aria-label={t('tag_content_search')}
                    onKeyDown={event => {
                        if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) return;
                        if (event.key === 'Escape' && library.query) { event.stopPropagation(); library.setQuery(''); }
                    }} />
                {library.query && <button type="button" className="btn-icon tag-manager-clear" onClick={() => library.setQuery('')} aria-label={t('tag_clear_search')}><X size={13} /></button>}
            </div>
            {managementMode && <div className="tag-manager-batch-toolbar">
                <button type="button" className="btn-icon tag-manager-text-button" disabled={busy || library.loading || library.error || library.total === 0} onClick={selectAll}
                    aria-pressed={selection.mode === 'all' && selection.excludedIds.length === 0}>
                    {t(selection.mode === 'all' && selection.excludedIds.length === 0 ? 'tag_clear_selection' : library.query ? 'tag_select_all_matches' : 'tag_select_all')}
                </button>
                <span role="status" aria-live="polite">{t('tag_selected_count').replace('{count}', String(selectedCount))}</span>
                <button type="button" className="btn-icon tag-manager-text-button tag-manager-batch-delete" disabled={busy || selectedCount === 0 || !selectedTag}
                    onClick={() => { if (selectedTag) openDialog({ kind: 'deleteSelection', tag: selectedTag, selection, count: selectedCount, scope: selectionScope }); }}>{t('common_delete')}</button>
            </div>}
            <div className="history-list-container tag-manager-results" aria-busy={library.loading}>
                {library.loading || (library.metadataLoading && library.tags.length === 0) ? <div className="tag-manager-empty" role="status"><LoaderCircle className="tag-manager-spinner" size={22} /></div>
                    : library.items.length ? <div className="virtual-list-wrapper"><Virtuoso ref={list} key={`${selectedTag}:${library.query}:${library.sortBy}`} data={library.items}
                        computeItemKey={(_, item) => item.id} endReached={library.loadMore} increaseViewportBy={100}
                        itemContent={(index, item) => <TagHistoryRow key={`${item.id}:${library.contentRevision}:${detailRevision}`} item={item} index={index} selected={selectedId === item.id}
                            compactMode={compactMode} isWindowPinned={isWindowPinned}
                            managementMode={managementMode} selectedForDeletion={managementMode && Number.isSafeInteger(item.id) && item.id > 0 && tagEntrySelected(selection, item.id)} onToggleSelection={toggleEntry}
                            richTextSnapshotPreview={richTextSnapshotPreview} theme={theme} language={language} t={t} tagColors={colors}
                            onSelect={setSelectedId} onEdit={editItem} onDelete={deleteItem} onQRCode={setQrEntry} onChanged={changed} onError={notifyError} />}
                        components={{ Footer: () => library.loadingMore ? <div className="tag-manager-loading"><LoaderCircle size={15} className="tag-manager-spinner" /></div> : null }} /></div>
                        : <div className="tag-manager-empty">{t(library.error || library.metadataError ? 'tag_load_failed' : !selectedTag ? 'tag_library_empty' : library.query ? 'tag_no_matching_entries' : 'tag_no_entries')}</div>}
            </div>
            {(batchFeedback || actionError) && <div className="tag-manager-notice" role={batchFeedback && !batchFeedback.error ? 'status' : 'alert'}>{batchFeedback?.message ?? t('tag_action_failed')}
                <button type="button" className="btn-icon" aria-label={t('common_close')} onClick={() => { setActionError(false); setBatchFeedback(null); }}><X size={13} /></button></div>}
        </section>
        {dialog && <TagDialog title={dialogTitle} closeLabel={t('common_close')} busy={busy} onClose={closeDialog} wide={isTextDialog}>
            <form onSubmit={event => { event.preventDefault(); void saveDialog(); }}>
                {isNameDialog && <label className="tag-manager-field"><span>{t('tag_name')}</span><input className="search-input" autoFocus value={value} maxLength={80} readOnly={busy} onChange={event => setValue(event.target.value)} /></label>}
                {isTextDialog && <label className="tag-manager-field" data-privacy-protected={protectedDialog ? 'true' : undefined}
                    onDragStart={event => { if (protectedDialog) event.preventDefault(); }}><span>{t('input_content_placeholder')}</span><textarea className="modal-textarea" autoFocus value={value} readOnly={busy} onChange={event => setValue(event.target.value)} spellCheck={false} /></label>}
                {isTextDialog && privacyActive && <>
                    <label className="privacy-manual-confirm"><input type="checkbox" checked={confirmPersistence} disabled={busy}
                        onChange={event => setConfirmPersistence(event.target.checked)} /><span>{t(dialog.kind === 'editItem' ? 'privacy_manual_edit_confirm' : 'privacy_manual_confirm')}</span></label>
                    <p className="privacy-manual-hint">{t(dialog.kind === 'editItem' ? 'privacy_manual_edit_hint' : 'privacy_manual_hint')}</p>
                </>}
                {dialog.kind === 'removeTag' && <p className="confirm-dialog-message">{t(destructiveTag ? 'tag_sensitive_delete_hint' : 'tag_remove_hint')}</p>}
                {dialog.kind === 'deleteItem' && <p className="confirm-dialog-message">{t('confirm_delete_desc')}</p>}
                {dialog.kind === 'deleteSelection' && <p className="confirm-dialog-message">{t(dialog.selection.mode === 'all' ? 'tag_batch_delete_all_confirm' : 'tag_batch_delete_confirm')
                    .replace('{count}', String(dialog.count)).replace('{tag}', dialog.tag)}</p>}
                {dialogError && <p className="tag-manager-dialog-error" role="alert">{dialogError}</p>}
                <div className="confirm-dialog-buttons">
                    <button type="button" className="confirm-dialog-button" disabled={busy} onClick={closeDialog}>{t('cancel')}</button>
                    <button type="submit" className="confirm-dialog-button primary" disabled={busy || ((isNameDialog || isTextDialog) && !value.trim())
                        || (isTextDialog && (privacy.phase !== 'ready' || (privacyActive && !confirmPersistence)))}>
                        {t(dialog.kind === 'deleteItem' || dialog.kind === 'deleteSelection' || destructiveTag ? 'common_delete' : dialog.kind === 'removeTag' ? 'tag_remove' : dialog.kind === 'createTag' ? 'tag_create' : 'common_save')}
                    </button>
                </div>
            </form>
        </TagDialog>}
        {qrEntry && <Suspense fallback={null}><QrCodeDialog entry={qrEntry} onClose={() => setQrEntry(null)} /></Suspense>}
    </div>;
}
