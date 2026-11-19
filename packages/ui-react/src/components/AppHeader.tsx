import { startTransition, Suspense, useEffect, useId, useRef } from "react";
import type { KeyboardEvent, ReactNode, RefObject } from "react";
import { AnimatePresence, motion } from "framer-motion";
import {
  ChevronLeft,
  Pin,
  PinOff,
  Search,
  Settings as SettingsIcon,
  Smile,
  Tag,
  Trash2,
  X
} from "lucide-react";


export interface TagFilterHandle {
  handleInputKeyDown: (event: KeyboardEvent<HTMLInputElement>) => void;
}

export interface HeaderTagFilter {
  ref: RefObject<TagFilterHandle | null>;
  id: string;
  query: string;
  isComposing: boolean;
  inputRef: RefObject<HTMLInputElement | null>;
  onClose: () => void;
  onSelect: (tag: string) => void;
}

export interface AppHeaderProps {
  desktopInteractions: boolean;
  pending: boolean;
  failed: boolean;
  onPinChange: (pinned: boolean) => void;
  onHide: () => void;
  onActivate?: () => void;
  dragAttributes?: Record<string, string | boolean>;
  renderTagFilter?: (props: HeaderTagFilter) => ReactNode;
  contentTypes: readonly { id: string; label: string }[];
  t: (key: string) => string;
  showSettings: boolean;
  setShowSettings: (val: boolean) => void;
  showTagManager: boolean;
  setShowTagManager: (val: boolean) => void;
  tagManagerEnabled: boolean;
  showEmojiPanel: boolean;
  setShowEmojiPanel: (val: boolean) => void;
  emojiPanelEnabled: boolean;
  isWindowPinned: boolean;
  clearHistory: () => void;
  showSearchBox: boolean;
  search: string;
  setSearch: (val: string) => void;
  isComposing: boolean;
  setIsComposing: (val: boolean) => void;
  searchInputRef: RefObject<HTMLInputElement | null>;
  showTagFilter: boolean;
  setShowTagFilter: (val: boolean) => void;
  searchIsFocused: boolean;
  setSearchIsFocused: (val: boolean) => void;
  setEditingTagsId: (val: number | null) => void;
  typeFilter: string | null;
  setTypeFilter: (val: string | null) => void;
}

const AppHeader = ({
  desktopInteractions, pending, failed, onPinChange, onHide, onActivate,
  dragAttributes, renderTagFilter, contentTypes,
  t,
  showSettings,
  setShowSettings,
  showTagManager,
  setShowTagManager,
  tagManagerEnabled,
  showEmojiPanel,
  setShowEmojiPanel,
  emojiPanelEnabled,
  isWindowPinned,
  clearHistory,
  showSearchBox,
  search,
  setSearch,
  isComposing,
  setIsComposing,
  searchInputRef,
  showTagFilter,
  setShowTagFilter,
  searchIsFocused,
  setSearchIsFocused,
  setEditingTagsId,
  typeFilter,
  setTypeFilter
}: AppHeaderProps) => {
  const panelOnTop = showSettings || showTagManager || showEmojiPanel;
  useEffect(() => {
    if (panelOnTop) setIsComposing(false);
  }, [panelOnTop, setIsComposing]);
  const searchBoxVisible = showSearchBox || search.trim().length > 0;
  // The whole search box sits behind a panel guard, so closing a panel remounts
  // it and replays the height reveal. That reveal animates height, which re-lays
  // out the list underneath on every frame; measured on a settings open+close it
  // was more than half the frame cost, for a transition the panel transition has
  // already covered. The reveal therefore only plays when the user is the one who
  // asked for the box, and a panel switch brings it back outright.
  const visibleBeforePanel = useRef(searchBoxVisible);
  useEffect(() => {
    if (!panelOnTop) visibleBeforePanel.current = searchBoxVisible;
  }, [searchBoxVisible, panelOnTop]);
  const revealSearchBox = searchBoxVisible && !visibleBeforePanel.current;
  const tagFilter = useRef<TagFilterHandle>(null);
  const tagFilterId = useId();
  const blurTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const tagFilterVisible = showTagFilter && searchIsFocused;
  useEffect(() => () => { if (blurTimer.current !== null) clearTimeout(blurTimer.current); }, []);

  return (
  <header {...dragAttributes}>
    <div className="header-top" {...dragAttributes}>
      <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
        {(showSettings || showTagManager || showEmojiPanel) && (
          <button className="btn-icon" aria-label={t("common_close")} onClick={() => {
            if (showEmojiPanel) setShowEmojiPanel(false);
            else if (showTagManager) setShowTagManager(false);
            else setShowSettings(false);
          }}>
            <ChevronLeft size={18} />
          </button>
        )}
        <span className="header-title">
          {showEmojiPanel
            ? (t('emoji_panel') || '表情包')
            : showTagManager && tagManagerEnabled
              ? (t('tag_manager') || '标签管理')
              : showSettings
                ? t('settings')
                : t('app_name')}
        </span>
      </div>
      <div style={{ display: 'flex', gap: '4px' }}>
        {/* Pin Button - Always visible but single instance */}
        <button
          className={`btn-icon btn-icon-scalable btn-icon-size-header ${isWindowPinned ? 'active' : ''}`}
          disabled={!desktopInteractions}
          title={t(desktopInteractions ? 'pin' : pending ? 'platform_capabilities_checking' : failed ? 'platform_capabilities_unavailable' : 'wayland_interaction_unavailable')}
          onClick={() => onPinChange(!isWindowPinned)}
        >
          {isWindowPinned ? <PinOff size={16} /> : <Pin size={16} />}
        </button>

        {!showSettings && !showTagManager && !showEmojiPanel && (
          <>
            <button className="btn-icon btn-icon-scalable btn-icon-size-header" title={t('clear_history')} onClick={clearHistory}>
              <Trash2 size={16} />
            </button>
            {tagManagerEnabled && (
              <button className="btn-icon btn-icon-scalable btn-icon-size-header" title={t('tag_manager') || '标签管理'} onClick={() => startTransition(() => {
                setShowTagManager(true);
                setShowSettings(false);
                setShowEmojiPanel(false);
              })}>
                <Tag size={16} />
              </button>
            )}
            {emojiPanelEnabled && (
              <button className="btn-icon btn-icon-scalable btn-icon-size-header" title={t('emoji_panel') || '表情包'} onClick={() => startTransition(() => {
                setShowEmojiPanel(true);
                setShowSettings(false);
                setShowTagManager(false);
              })}>
                <Smile size={16} />
              </button>
            )}
            <button className="btn-icon btn-icon-scalable btn-icon-size-header" title={t('settings')} onClick={() => startTransition(() => {
              setShowSettings(true);
              setShowTagManager(false);
              setShowEmojiPanel(false);
            })}>
              <SettingsIcon size={16} />
            </button>
          </>
        )}
        <button className="btn-icon btn-icon-scalable btn-icon-size-header" title={t('hide')} onClick={onHide}>
          <X size={16} />
        </button>
      </div>
    </div>

    {!showSettings && !showTagManager && !showEmojiPanel && (
      <AnimatePresence>
        {(showSearchBox || search.trim().length > 0) && (
          <motion.div
            initial={revealSearchBox ? { height: 0, opacity: 0, overflow: 'hidden' } : false}
            animate={{
              height: "auto",
              opacity: 1,
              transitionEnd: { overflow: "visible" }
            }}
            exit={{ height: 0, opacity: 0, overflow: 'hidden' }}
            transition={{ duration: 0.2, ease: "circOut" }}
            style={{ flexShrink: 0 }}
          >
            <div className="search-container">
              <div style={{ position: 'relative' }}>
                <Search size={14} className="search-icon" />
                <input
                  ref={searchInputRef}
                  type="text"
                  className={`search-input ${tagFilterVisible ? 'dropdown-open' : ''}`}
                  placeholder={t('search_placeholder')}
                  aria-label={t('search_placeholder')}
                  value={search}
                  role="combobox"
                  aria-autocomplete="list"
                  aria-expanded={tagFilterVisible}
                  aria-controls={tagFilterVisible ? tagFilterId : undefined}
                  onKeyDown={event => {
                    if (tagFilter.current) tagFilter.current.handleInputKeyDown(event);
                    else if (!event.nativeEvent.isComposing && tagFilterVisible && ['ArrowDown', 'ArrowUp', 'Enter', 'Escape'].includes(event.key)) {
                      event.preventDefault(); event.stopPropagation();
                      if (event.key === 'Escape') setShowTagFilter(false);
                    }
                  }}
                  onCompositionStart={() => setIsComposing(true)}
                  onCompositionEnd={(e) => {
                    setIsComposing(false);
                    setSearch((e.target as HTMLInputElement).value);
                  }}
                  onChange={(e) => {
                    setSearch(e.target.value);
                  }}
                  onMouseDown={onActivate}
                  onClick={() => { setShowTagFilter(true); setEditingTagsId(null); }}
                  onFocus={() => {
                    if (blurTimer.current !== null) clearTimeout(blurTimer.current);
                    onActivate?.();
                    setShowTagFilter(true);
                    setSearchIsFocused(true);
                    setEditingTagsId(null);
                  }}
                  onBlur={() => {
                    setIsComposing(false);
                    blurTimer.current = setTimeout(() => {
                      blurTimer.current = null;
                      if (document.activeElement?.closest('.dc-tag-filter-picker')) return;
                      setShowTagFilter(false);
                      setSearchIsFocused(false);
                    }, 200);
                  }}
                  style={{ color: 'var(--search-input-text, var(--text-primary))' }}
                />
                {tagFilterVisible && (
                  <Suspense fallback={<div className="dc-tag-filter-picker" id={tagFilterId} role="listbox" aria-label={t('tags')}><div className="dc-tag-filter-empty">{t('common_loading')}</div></div>}>
                    {renderTagFilter?.({
                      ref: tagFilter, id: tagFilterId, query: search, isComposing, inputRef: searchInputRef,
                      onClose: () => {
                        setShowTagFilter(false);
                        if (document.activeElement !== searchInputRef.current) setSearchIsFocused(false);
                      },
                      onSelect: tag => { setSearch("tag:" + tag); setShowTagFilter(false); },
                    })}
                  </Suspense>
                )}
              </div>
              <div
                className="hide-scrollbar"
                style={{
                  display: 'flex',
                  gap: '6px',
                  padding: '8px 0 4px 0',
                  overflowX: 'auto',
                  overflowY: 'visible',
                  scrollbarWidth: 'none',
                  msOverflowStyle: 'none'
                }}
                onWheel={(e) => {
                  if (e.deltaY !== 0) {
                    e.currentTarget.scrollLeft += e.deltaY;
                  }
                }}
              >
                {contentTypes.map(({ id: type, label }) => (
                  <button
                    key={type}
                    className={`btn-icon ${typeFilter === type ? 'active' : ''}`}
                    onClick={() => setTypeFilter(typeFilter === type ? null : type)}
                    style={{
                      width: 'auto',
                      padding: '4px 8px',
                      fontSize: '11px',
                      borderRadius: 'var(--icon-btn-radius, 4px)',
                      whiteSpace: 'nowrap',
                      flexShrink: 0,
                      opacity: typeFilter === type ? 1 : 0.7
                    }}
                    title={label}
                  >
                    {label}
                  </button>
                ))}
              </div>

            </div>
          </motion.div>
        )}
      </AnimatePresence>
    )}
  </header>
);
};

export default AppHeader;
