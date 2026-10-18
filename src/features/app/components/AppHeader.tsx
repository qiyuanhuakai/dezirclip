import { lazy, Suspense, useEffect, useId, useRef } from "react";
import type { RefObject } from "react";
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
import { invoke } from "@tauri-apps/api/core";
import type { TagFilterPickerHandle } from "../../tag/components/TagFilterPicker";
import { getClipboardTypeName } from "../../../shared/lib/clipboardTypeName";

const TagFilterPicker = lazy(() => import("../../tag/components/TagFilterPicker"));

interface AppHeaderProps {
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
  setIsWindowPinned: (val: boolean) => void;
  clearHistory: () => void;
  showSearchBox: boolean;
  search: string;
  setSearch: (val: string) => void;
  setIsComposing: (val: boolean) => void;
  searchInputRef: RefObject<HTMLInputElement | null>;
  showTagFilter: boolean;
  setShowTagFilter: (val: boolean) => void;
  allTags: string[];
  tagColors?: Record<string, string>;
  searchIsFocused: boolean;
  setSearchIsFocused: (val: boolean) => void;
  setEditingTagsId: (val: number | null) => void;
  theme: string;
  typeFilter: string | null;
  setTypeFilter: (val: string | null) => void;
}

const AppHeader = ({
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
  setIsWindowPinned,
  clearHistory,
  showSearchBox,
  search,
  setSearch,
  setIsComposing,
  searchInputRef,
  showTagFilter,
  setShowTagFilter,
  allTags,
  tagColors = {},
  searchIsFocused,
  setSearchIsFocused,
  setEditingTagsId,
  theme,
  typeFilter,
  setTypeFilter
}: AppHeaderProps) => {
  const panelOnTop = showSettings || showTagManager || showEmojiPanel;
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
  const tagFilter = useRef<TagFilterPickerHandle>(null);
  const tagFilterId = useId();
  const blurTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const tagFilterVisible = showTagFilter && searchIsFocused;
  useEffect(() => () => { if (blurTimer.current !== null) clearTimeout(blurTimer.current); }, []);

  return (
  <header data-tauri-drag-region>
    <div className="header-top" data-tauri-drag-region>
      <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
        {(showSettings || showTagManager || showEmojiPanel) && (
          <button className="btn-icon" onClick={() => {
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
          title={t('pin')}
          onClick={() => {
            const newVal = !isWindowPinned;
            setIsWindowPinned(newVal);
            invoke("set_window_pinned", { pinned: newVal }).catch(console.error);
          }}
        >
          {isWindowPinned ? <PinOff size={16} /> : <Pin size={16} />}
        </button>

        {!showSettings && !showTagManager && !showEmojiPanel && (
          <>
            <button className="btn-icon btn-icon-scalable btn-icon-size-header" title={t('clear_history')} onClick={clearHistory}>
              <Trash2 size={16} />
            </button>
            {tagManagerEnabled && (
              <button className="btn-icon btn-icon-scalable btn-icon-size-header" title={t('tag_manager') || '标签管理'} onClick={() => setShowTagManager(true)}>
                <Tag size={16} />
              </button>
            )}
            {emojiPanelEnabled && (
              <button className="btn-icon btn-icon-scalable btn-icon-size-header" title={t('emoji_panel') || '表情包'} onClick={() => setShowEmojiPanel(true)}>
                <Smile size={16} />
              </button>
            )}
            <button className="btn-icon btn-icon-scalable btn-icon-size-header" title={t('settings')} onClick={() => setShowSettings(true)}>
              <SettingsIcon size={16} />
            </button>
          </>
        )}
        <button className="btn-icon btn-icon-scalable btn-icon-size-header" title={t('hide')} onClick={async () => {
          invoke("hide_window_cmd").catch(console.error);
        }}>
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
                  onMouseDown={() => {
                    invoke("activate_window_focus").catch(console.error);
                  }}
                  onClick={() => { setShowTagFilter(true); setEditingTagsId(null); }}
                  onFocus={() => {
                    if (blurTimer.current !== null) clearTimeout(blurTimer.current);
                    invoke("activate_window_focus").catch(console.error);
                    setShowTagFilter(true);
                    setSearchIsFocused(true);
                    setEditingTagsId(null);
                  }}
                  onBlur={() => {
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
                    <TagFilterPicker ref={tagFilter} id={tagFilterId} tags={allTags} colors={tagColors} query={search} theme={theme}
                      inputRef={searchInputRef} t={t} onClose={() => {
                        setShowTagFilter(false);
                        if (document.activeElement !== searchInputRef.current) setSearchIsFocused(false);
                      }}
                      onSelect={tag => { setSearch("tag:" + tag); setShowTagFilter(false); }} />
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
                {['text', 'image', 'file', 'url', 'code', 'video', 'rich_text'].map(type => (
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
                    title={getClipboardTypeName(type, t)}
                  >
                    {getClipboardTypeName(type, t)}
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
