import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { Virtuoso, type VirtuosoHandle } from 'react-virtuoso';
import { ArrowDownWideNarrow, ChevronLeft, ChevronRight, Edit2, List, ListCollapse, LoaderCircle, MoreHorizontal, Plus, Search, Tag, Trash2, X } from 'lucide-react';
import type { ClipboardEntry, Locale } from '../../../shared/types';
import { errorMessage } from '../../../shared/lib/errorMessage';
import { useTagLibrary } from '../hooks/useTagLibrary';
import { TagDialog } from './TagDialog';
import { TagHistoryRow } from './TagHistoryRow';

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
    | { kind: 'createItem'; tag: string }
    | { kind: 'editItem'; id: number }
    | { kind: 'deleteItem'; item: ClipboardEntry };

function protectedTag(name: string): boolean { return name.toLowerCase() === 'sensitive' || name === '密码'; }

export default function TagManager({ t, theme, language = 'zh', compactMode = false, isWindowPinned = false,
    privacyProtection = true, tagColors, richTextSnapshotPreview = false, onCompactModeChange }: TagManagerProps) {
    const library = useTagLibrary();
    const { selectedTag, selectTag, refresh } = library;
    const [tagSearch, setTagSearch] = useState('');
    const [collapsed, setCollapsed] = useState(() => window.innerWidth < 500);
    const [menuOpen, setMenuOpen] = useState(false);
    const [selectedId, setSelectedId] = useState<number | null>(null);
    const [detailRevision, setDetailRevision] = useState(0);
    const [dialog, setDialog] = useState<Dialog | null>(null);
    const [qrEntry, setQrEntry] = useState<ClipboardEntry | null>(null);
    const [value, setValue] = useState('');
    const [busy, setBusy] = useState(false);
    const [dialogError, setDialogError] = useState<string | null>(null);
    const [actionError, setActionError] = useState(false);
    const root = useRef<HTMLDivElement>(null);
    const menu = useRef<HTMLDivElement>(null);
    const menuButton = useRef<HTMLButtonElement>(null);
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

    useEffect(() => {
        mounted.current = true;
        return () => { mounted.current = false; detailSequence.current += 1; focusSequence.current += 1; };
    }, []);
    useEffect(() => { detailSequence.current += 1; setActionError(false); setSelectedId(null); setMenuOpen(false); }, [selectedTag, library.query, library.sortBy]);
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

    const openDialog = useCallback((next: Dialog) => {
        detailSequence.current += 1;
        setMenuOpen(false); setDialogError(null);
        setValue(next.kind === 'renameTag' ? next.tag : next.kind === 'createTag' ? tagSearch.trim() : '');
        setDialog(next);
    }, [tagSearch]);
    const editItem = useCallback((item: ClipboardEntry) => {
        const request = ++detailSequence.current;
        void invoke<ClipboardEntry>('get_tag_item_detail', { id: item.id }).then(detail => {
            if (!mounted.current || request !== detailSequence.current) return;
            setDialogError(null); setValue(detail.content); setDialog({ kind: 'editItem', id: item.id });
        }).catch(() => { if (request === detailSequence.current) notifyError(); });
    }, [notifyError]);
    const deleteItem = useCallback((item: ClipboardEntry) => openDialog({ kind: 'deleteItem', item }), [openDialog]);
    const closeDialog = () => { if (!busy) setDialog(null); };

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
        const trimmed = value.trim();
        if (['createTag', 'renameTag', 'createItem', 'editItem'].includes(current.kind) && !trimmed) return;
        if (current.kind === 'renameTag' && protectedTag(trimmed)) { setDialogError(t('tag_sensitive_target')); return; }
        if (current.kind === 'removeTag' && protectedTag(current.tag) && privacyProtection) { setDialogError(t('tag_sensitive_delete_locked')); return; }
        setBusy(true); setDialogError(null);
        try {
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
                await invoke('add_manual_item', { content: value, contentType: 'text', tags: [current.tag] });
            } else if (current.kind === 'editItem') {
                await invoke('update_item_content', { id: current.id, newContent: value });
            } else await invoke('delete_clipboard_entry', { id: current.item.id });
            if (!mounted.current) return;
            setDialog(null); changed();
            void emit('clipboard-changed').catch(console.error);
        } catch (reason) {
            if (mounted.current) setDialogError(current.kind === 'removeTag' && protectedTag(current.tag)
                ? errorMessage(reason) || t('tag_action_failed') : t('tag_action_failed'));
        }
        finally { if (mounted.current) setBusy(false); }
    };

    const handleKeyboard = (event: KeyboardEvent<HTMLDivElement>) => {
        const target = event.target as HTMLElement;
        if (target.closest('dialog')) return;
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'f') {
            event.preventDefault(); searchInput.current?.focus(); return;
        }
        if (target.closest('input, textarea, [role="menu"]')) return;
        if (event.key === 'F2' && selectedTag && !protectedTag(selectedTag)) {
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
    const isNameDialog = dialog?.kind === 'createTag' || dialog?.kind === 'renameTag';
    const destructiveTag = dialog?.kind === 'removeTag' && protectedTag(dialog.tag);
    const dialogTitle = dialog?.kind === 'createTag' ? t('tag_create') : dialog?.kind === 'renameTag' ? t('tag_rename')
        : dialog?.kind === 'removeTag' ? destructiveTag ? t('tag_sensitive_delete_title') : t('tag_remove_title').replace('{tag}', dialog.tag)
            : dialog?.kind === 'createItem' ? t('add_item') : dialog?.kind === 'editItem' ? t('edit_item') : t('confirm_delete');
    const menuItem = (icon: ReactNode, label: string, action: () => void, disabled = false) => <button type="button"
        className="item-context-menu__item" role="menuitem" disabled={disabled}
        onClick={() => { setMenuOpen(false); action(); }}><span className="item-context-menu__item-label">{icon}{label}</span></button>;

    return <div ref={root} className={`tag-manager${collapsed ? ' tag-manager--collapsed' : ''}`} onKeyDown={handleKeyboard}
        onPointerDownCapture={() => { void invoke('activate_window_focus').catch(console.error); }}>
        <aside className="settings-rail tag-manager-rail" aria-label={t('tags')}>
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
                <Virtuoso ref={tagList} data={filteredTags} fixedItemHeight={38} computeItemKey={(_, tag) => tag.name}
                    itemContent={(index, tag) => <div className="tag-manager-tag-cell"><button type="button" data-tag-index={index}
                        className={`settings-rail-item tag-manager-tag${selectedTag === tag.name ? ' is-active' : ''}`}
                        onClick={() => selectTag(tag.name)} aria-label={`${tag.name} · ${tag.count}`} aria-current={selectedTag === tag.name ? 'page' : undefined} title={`${tag.name} · ${tag.count}`}>
                        {collapsed ? <span className="tag-manager-tag-initial" aria-hidden="true">{Array.from(tag.name)[0]}</span>
                            : <><span className="tag-manager-tag-name">{tag.name}</span><small>{tag.count}</small></>}
                    </button></div>} />
            </div>
            {collapsed && <button type="button" className="btn-icon tag-manager-rail-add" title={t('tag_create')} aria-label={t('tag_create')} onClick={() => openDialog({ kind: 'createTag' })}><Plus size={16} /></button>}
        </aside>
        <section className="tag-manager-content" aria-label={selectedTag ?? t('tag_manager')}>
            <div className="tag-manager-toolbar">
                <span className="tag-manager-current" title={selectedTag ?? ''}><Tag size={14} /><span>{selectedTag ?? t('tags')}</span></span>
                <button type="button" className={`btn-icon${compactMode ? ' active' : ''}`} onClick={() => onCompactModeChange?.(!compactMode)}
                    disabled={!onCompactModeChange} title={t(compactMode ? 'tag_mode_detailed' : 'tag_mode_simple')}
                    aria-label={t(compactMode ? 'tag_mode_detailed' : 'tag_mode_simple')} aria-pressed={compactMode}>
                    {compactMode ? <List size={16} /> : <ListCollapse size={16} />}
                </button>
                <button type="button" className="btn-icon" disabled={!selectedTag} title={t('add_item')} aria-label={t('add_item')}
                    onClick={() => { if (selectedTag) openDialog({ kind: 'createItem', tag: selectedTag }); }}><Plus size={16} /></button>
                <button ref={menuButton} type="button" className={`btn-icon${menuOpen ? ' active' : ''}`} title={t('tag_actions')} aria-label={t('tag_actions')}
                    disabled={!selectedTag} aria-haspopup="menu" aria-expanded={menuOpen} onClick={() => setMenuOpen(current => !current)}><MoreHorizontal size={17} /></button>
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
                    onKeyDown={event => { if (event.key === 'Escape' && library.query) { event.stopPropagation(); library.setQuery(''); } }} />
                {library.query && <button type="button" className="btn-icon tag-manager-clear" onClick={() => library.setQuery('')} aria-label={t('tag_clear_search')}><X size={13} /></button>}
            </div>
            <div className="history-list-container tag-manager-results" aria-busy={library.loading}>
                {library.loading || (library.metadataLoading && library.tags.length === 0) ? <div className="tag-manager-empty" role="status"><LoaderCircle className="tag-manager-spinner" size={22} /></div>
                    : library.items.length ? <div className="virtual-list-wrapper"><Virtuoso ref={list} key={`${selectedTag}:${library.query}:${library.sortBy}`} data={library.items}
                        computeItemKey={(_, item) => item.id} endReached={library.loadMore} increaseViewportBy={100}
                        itemContent={(index, item) => <TagHistoryRow key={`${item.id}:${library.contentRevision}:${detailRevision}`} item={item} index={index} selected={selectedId === item.id}
                            compactMode={compactMode} isWindowPinned={isWindowPinned} privacyProtection={privacyProtection}
                            richTextSnapshotPreview={richTextSnapshotPreview} theme={theme} language={language} t={t} tagColors={colors}
                            onSelect={setSelectedId} onEdit={editItem} onDelete={deleteItem} onQRCode={setQrEntry} onChanged={changed} onError={notifyError} />}
                        components={{ Footer: () => library.loadingMore ? <div className="tag-manager-loading"><LoaderCircle size={15} className="tag-manager-spinner" /></div> : null }} /></div>
                        : <div className="tag-manager-empty">{t(library.error || library.metadataError ? 'tag_load_failed' : !selectedTag ? 'tag_library_empty' : library.query ? 'tag_no_matching_entries' : 'tag_no_entries')}</div>}
            </div>
            {actionError && <div className="tag-manager-notice" role="alert">{t('tag_action_failed')}<button type="button" className="btn-icon" aria-label={t('common_close')} onClick={() => setActionError(false)}><X size={13} /></button></div>}
        </section>
        {dialog && <TagDialog title={dialogTitle} closeLabel={t('common_close')} busy={busy} onClose={closeDialog} wide={isTextDialog}>
            <form onSubmit={event => { event.preventDefault(); void saveDialog(); }}>
                {isNameDialog && <label className="tag-manager-field"><span>{t('tag_name')}</span><input className="search-input" autoFocus value={value} maxLength={80} readOnly={busy} onChange={event => setValue(event.target.value)} /></label>}
                {isTextDialog && <label className="tag-manager-field"><span>{t('input_content_placeholder')}</span><textarea className="modal-textarea" autoFocus value={value} readOnly={busy} onChange={event => setValue(event.target.value)} spellCheck={false} /></label>}
                {dialog.kind === 'removeTag' && <p className="confirm-dialog-message">{t(destructiveTag ? 'tag_sensitive_delete_hint' : 'tag_remove_hint')}</p>}
                {dialog.kind === 'deleteItem' && <p className="confirm-dialog-message">{t('confirm_delete_desc')}</p>}
                {dialogError && <p className="tag-manager-dialog-error" role="alert">{dialogError}</p>}
                <div className="confirm-dialog-buttons">
                    <button type="button" className="confirm-dialog-button" disabled={busy} onClick={closeDialog}>{t('cancel')}</button>
                    <button type="submit" className="confirm-dialog-button primary" disabled={busy || ((isNameDialog || isTextDialog) && !value.trim())}>
                        {t(dialog.kind === 'deleteItem' || destructiveTag ? 'common_delete' : dialog.kind === 'removeTag' ? 'tag_remove' : dialog.kind === 'createTag' ? 'tag_create' : 'common_save')}
                    </button>
                </div>
            </form>
        </TagDialog>}
        {qrEntry && <Suspense fallback={null}><QrCodeDialog entry={qrEntry} onClose={() => setQrEntry(null)} /></Suspense>}
    </div>;
}
