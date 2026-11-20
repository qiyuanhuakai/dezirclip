import { startTransition, useEffect, useRef } from "react";
import { ClipboardSearch, type ClipboardSearchProps } from "./ClipboardSearch";

export type { HeaderTagFilter, TagFilterHandle } from "./ClipboardSearch";
import { AnimatePresence, motion } from "framer-motion";
import {
  ChevronLeft,
  Pin,
  PinOff,
  Settings as SettingsIcon,
  Smile,
  Tag,
  Trash2,
  X
} from "lucide-react";


export interface AppHeaderProps extends ClipboardSearchProps {
  desktopInteractions: boolean;
  pending: boolean;
  failed: boolean;
  onPinChange: (pinned: boolean) => void;
  onHide: () => void;
  dragAttributes?: Record<string, string | boolean>;
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
            <ClipboardSearch t={t} onActivate={onActivate} renderTagFilter={renderTagFilter} contentTypes={contentTypes}
              search={search} setSearch={setSearch} isComposing={isComposing} setIsComposing={setIsComposing}
              searchInputRef={searchInputRef} showTagFilter={showTagFilter} setShowTagFilter={setShowTagFilter}
              searchIsFocused={searchIsFocused} setSearchIsFocused={setSearchIsFocused} setEditingTagsId={setEditingTagsId}
              typeFilter={typeFilter} setTypeFilter={setTypeFilter} />
          </motion.div>
        )}
      </AnimatePresence>
    )}
  </header>
);
};

export default AppHeader;
