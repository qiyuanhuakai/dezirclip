import { useRef, useState } from "react";
import { Pin, Tag, X } from "lucide-react";
import { createRoot } from "react-dom/client";
import { AppHeader, CharacterSearch, ClipboardSearch, ConfirmDialog, EmojiGrid, EmojiTabs, FilterChips, HistoryCard, HistoryCardAction, HistoryCardMeta, HotkeyBinding, MatchHighlight,
  ProgressToast, ScrollToTopButton, SearchBar, SearchHistory, SettingHelp, SettingsDisclosure, SettingsFooter, SettingsRail,
  SettingsSlider, SettingsToggle, TagDialog, ThemedSelect, ToastContainer, VirtualClipboardList, type EmojiTab, type SearchMode } from "../src";
import { isBuiltinTheme, loadBuiltinTheme } from "../src/themes";
import { nativeUiPalette } from "../src/native-appearance";
import { AttachmentFixture, PreviewImageFixture, PreviewToolbarFixture, ResizeFixture, WheelFixture } from "./preview-fixtures";
import { ContextMenuFixture, EmojiFavoritesFixture, HotkeyListFixture, SettingsChoicesFixture,
  SettingsDataFixture, SettingsRemoveFixture, SettingsSectionFixture, SettingsSectionGridFixture, TagToolbarFixture, TextFieldsFixture } from "./form-menu-fixtures";
import "../src/base.css";
import "../src/styles/components/index.css";
import "./showcase.css";

const labels = [
  { id: "text", label: "Text" }, { id: "image", label: "Image" }, { id: "file", label: "File" },
];
const options = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "disabled", label: "Unavailable", disabled: true },
];
const params = new URLSearchParams(location.search);
const requested = params.get("theme") ?? "mica";
const theme = isBuiltinTheme(requested) ? requested : "mica";
const mode = params.get("mode") === "dark" ? "dark" : "light";
const fixture = params.get("fixture") ?? "gallery";
const state = params.get("state") ?? "default";

const clipboardTypes = [
  { id: "text", label: "文本" }, { id: "image", label: "图片" }, { id: "file", label: "文件" },
  { id: "url", label: "URL" }, { id: "code", label: "代码" }, { id: "video", label: "视频" }, { id: "rich_text", label: "富文本" },
];

function ClipboardSearchFixture() {
  const [search, setSearch] = useState("");
  const [isComposing, setIsComposing] = useState(false);
  const [showTagFilter, setShowTagFilter] = useState(false);
  const [searchIsFocused, setSearchIsFocused] = useState(false);
  const [typeFilter, setTypeFilter] = useState<string | null>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  return <ClipboardSearch t={() => "搜索剪切板..."} search={search} setSearch={setSearch} contentTypes={clipboardTypes}
    isComposing={isComposing} setIsComposing={setIsComposing} showTagFilter={showTagFilter} setShowTagFilter={setShowTagFilter}
    searchIsFocused={searchIsFocused} setSearchIsFocused={setSearchIsFocused} searchInputRef={searchInputRef}
    setEditingTagsId={() => undefined} typeFilter={typeFilter} setTypeFilter={setTypeFilter} />;
}

function HeaderFixture() {
  const [settings, setSettings] = useState(state === "settings");
  const [tags, setTags] = useState(false);
  const [emoji, setEmoji] = useState(false);
  const [pinned, setPinned] = useState(state === "pinned");
  const [search, setSearch] = useState("");
  const [isComposing, setIsComposing] = useState(false);
  const [tagFilter, setTagFilter] = useState(false);
  const [focused, setFocused] = useState(false);
  const [typeFilter, setTypeFilter] = useState<string | null>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const language: Record<string, string> = {
    app_name: "DezirClip", settings: "Settings", search_placeholder: "Search clipboard...",
    common_close: "Back", hide: "Hide", pin: "Pin", tag_manager: "Tag Manager", emoji_panel: "Emoji",
    clear_history: "Clear History", tags: "Tags", common_loading: "Loading",
  };
  return <AppHeader t={key => language[key] ?? key} desktopInteractions pending={false} failed={false}
    onPinChange={setPinned} onHide={() => undefined} showSettings={settings} setShowSettings={setSettings}
    showTagManager={tags} setShowTagManager={setTags} tagManagerEnabled showEmojiPanel={emoji}
    setShowEmojiPanel={setEmoji} emojiPanelEnabled isWindowPinned={pinned} clearHistory={() => undefined}
    showSearchBox search={search} setSearch={setSearch} isComposing={isComposing} setIsComposing={setIsComposing}
    searchInputRef={searchInputRef} showTagFilter={tagFilter} setShowTagFilter={setTagFilter}
    searchIsFocused={focused} setSearchIsFocused={setFocused} setEditingTagsId={() => undefined}
    typeFilter={typeFilter} setTypeFilter={setTypeFilter} contentTypes={labels} />;
}

const settingsGroups = [
  { id: "appearance", labelKey: "Appearance" },
  { id: "general", labelKey: "General" },
  { id: "updates", labelKey: "Software updates" },
];
const footerLabels: Record<string, string> = {
  feedback: "Feedback", reset_defaults: "Reset defaults", official_website: "Official website",
};

const historyRows = [
  { id: 1, source: "Editor", time: "12:30", summary: "Copied from the active editor" },
  { id: 2, source: "Browser", time: "12:28", summary: "Release checklist and verification notes" },
  { id: 3, source: "Terminal", time: "12:25", summary: "Preview package ready for testing" },
];

function HistoryListFixture() {
  const [selected, select] = useState(state === "selected" ? 1 : -1);
  return <div className="history-list-container showcase-history-list">
    <VirtualClipboardList items={historyRows} pinnedIds={[]} onReorder={() => undefined}
      draggingPinned={false} compactMode={false} hasMore={false} isLoading={false}
      selectedIndex={selected} isKeyboardMode={state === "selected"}
      renderItem={(row, index) => <HistoryCard selected={index === selected} disableLayout onClick={() => select(index)}>
        <HistoryCardMeta source={row.source} time={row.time} actions={<>
          <HistoryCardAction title="Pin" active={state.includes("active")}><Pin size={12} /></HistoryCardAction>
          <HistoryCardAction title="Tags"><Tag size={12} /></HistoryCardAction>
          <HistoryCardAction title="Delete"><X size={12} /></HistoryCardAction>
        </>} />
        <div className="content-preview">{row.summary}</div>
      </HistoryCard>} />
  </div>;
}

function Showcase() {
  const [activeSettings, setActiveSettings] = useState(state === "selected" ? "updates" : "appearance");
  const tagName = useRef<HTMLInputElement>(null);
  const [selection, setSelection] = useState("system");
  const [query, setQuery] = useState(state === "filled" ? "Clipboard" : "");
  const [searchMode, setSearchMode] = useState<SearchMode>("fts");
  const [filters, setFilters] = useState(["text"]);
  const [opened, setOpened] = useState(true);
  const [toastVisible, setToastVisible] = useState(true);
  const [enabled, setEnabled] = useState(state === "enabled");
  const [opacity, setOpacity] = useState(72);
  const [tab, setTab] = useState<EmojiTab>(state === "favorites" ? "favorites" : "emoji");

  const examples = {
    "text-fields": <TextFieldsFixture />,
    "context-menu": <ContextMenuFixture state={state} />,
    "settings-section-card": state === "wide" ? <SettingsSectionGridFixture /> : <SettingsSectionFixture open={state === "expanded"} />,
    "settings-choices": <SettingsChoicesFixture />,
    "hotkey-list": <HotkeyListFixture multiple={state === "multiple"} />,
    "emoji-favorites": <EmojiFavoritesFixture />,
    "settings-data": <SettingsDataFixture />,
    "settings-remove": <SettingsRemoveFixture />,
    "tag-toolbar": <TagToolbarFixture />,
    "attachment-preview": <AttachmentFixture state={state} />,
    "preview-toolbar": <PreviewToolbarFixture state={state} />,
    "preview-image": <PreviewImageFixture state={state} />,
    "preview-resize-handles": <ResizeFixture />,
    "wheel-hot-zone": <WheelFixture />,
    "history-card-meta": <HistoryCard disableLayout><HistoryCardMeta source="Editor" time="12:30"
      sourceIcon={<Pin size={10} />} actions={<button type="button" className="btn-icon" title="Tags"><Tag size={12} /></button>} /></HistoryCard>,
    "history-list": <HistoryListFixture />,
    "settings-rail": <SettingsRail t={key => key} groups={settingsGroups} activeGroup={activeSettings} onSelect={setActiveSettings} />,
    "settings-footer": <SettingsFooter t={key => footerLabels[key] ?? key} productName="DezirClip 0.2"
      compact={state === "compact"} onFeedback={() => undefined} onWebsite={() => undefined} onResetSettings={() => undefined} />,
    "settings-toggle": <div className="setting-item"><span className="item-label">Keep clipboard history</span>
      <SettingsToggle aria-label="Keep clipboard history" checked={enabled} onChange={event => setEnabled(event.currentTarget.checked)} />
    </div>,
    "settings-slider": <div className="settings-view showcase-settings"><div className="setting-item column">
      <span className="item-label">Opacity {opacity}%</span>
      <SettingsSlider min={0} max={100} value={opacity} aria-label="Opacity" onChange={event => setOpacity(event.currentTarget.valueAsNumber)} />
    </div></div>,
    "hotkey-binding": <HotkeyBinding label="Quick Paste" value="Ctrl+Shift+V" placeholder="Set shortcut" recordingLabel="Press a shortcut"
      recording={state === "recording"} busy={false} disabled={false} onClick={undefined} onKeyDown={undefined}
      onMouseDown={undefined} onBlur={undefined} />,
    "scroll-to-top": <div className="showcase-scroll-top">Clipboard history<ScrollToTopButton label="Scroll to top" onClick={() => undefined} /></div>,
    "character-search": <CharacterSearch label="Search symbols" value={query} onChange={setQuery} />,
    "emoji-tabs": <EmojiTabs active={tab} labels={{ emoji: "Emoji", favorites: "Favorites" }} onChange={setTab} />,
    "emoji-grid": <EmojiGrid groups={[{ name: "Frequently used", emojis: ["😀", "😄", "😂", "🤣", "😊", "😍", "😘", "😎", "🤔", "🔥", "✨", "👌"] }]}
      empty="No emoji" onSelect={() => undefined} />,
    "app-header": <HeaderFixture />,
    "clipboard-search": <ClipboardSearchFixture />,
    "history-card": <HistoryCard selected={state === "selected"} pinned={state === "pinned"}
      compact={state.startsWith("compact")} disableLayout>
      <HistoryCardMeta source="Editor" time="12:30"
        sourceIcon={state === "pinned" ? <Pin size={10} /> : undefined}
        actions={<>
          <HistoryCardAction title="Pin" active={state.includes("active")}><Pin size={12} /></HistoryCardAction>
          <HistoryCardAction title="Tags"><Tag size={12} /></HistoryCardAction>
          <HistoryCardAction title="Delete"><X size={12} /></HistoryCardAction>
        </>} />
      <div className="content-preview">Copied from the active editor</div>
    </HistoryCard>,
    select: <ThemedSelect options={options} value={selection} onChange={setSelection}
      isDisabled={state === "disabled"} searchable ariaLabel="Color mode" />,
    "search-bar": <SearchBar query={query} setQuery={setQuery} mode={searchMode} setMode={setSearchMode}
      enabledModes={["fts", "fuzzy", "regex"]} resultCount={query ? 1 : 0} loading={state === "loading"}
      error={state === "error" ? "Search unavailable" : null} recentSearches={["Clipboard", "Documents"]}
      fetchRecentSearches={() => undefined} />,
    filters: <FilterChips kinds={labels} activeKinds={filters} counts={{ text: 7, image: 2, file: 1 }}
      allLabel="All" clearLabel="Clear" set={setFilters} clear={() => setFilters([])}
      toggle={id => setFilters(previous => previous.includes(id) ? previous.filter(item => item !== id) : [...previous, id])} />,
    highlight: <MatchHighlight text="Clipboard history" query="history" />,
    "search-history": <div className="showcase-search-history"><SearchHistory items={state === "empty" ? [] : ["Clipboard", "Documents"]}
      emptyLabel="No recent searches" onSelect={setQuery} /></div>,
    disclosure: <SettingsDisclosure title="Software updates">Preview channel</SettingsDisclosure>,
    help: <div className="settings-label-line"><SettingHelp helpLabel="Setting help" label="Software updates">
      Updates are checked when the app starts.
    </SettingHelp></div>,
    progress: <ProgressToast toasts={toastVisible ? [{ id: "sample", message: "Importing", progress: 0.6,
      status: state === "done" || state === "error" ? state : "running" }] : []}
      onDismiss={() => setToastVisible(false)} />,
    toast: <ToastContainer toasts={toastVisible ? [{ id: 1, msg: "Copied to clipboard" }] : []} />,
    confirm: <ConfirmDialog open={opened} title="Clear history" message="Clear all unpinned entries?"
      themeClass={"theme-" + theme} confirmLabel="Clear" cancelLabel="Cancel" onConfirm={() => setOpened(false)} onClose={() => setOpened(false)} />,
    "tag-dialog": opened && <TagDialog title="Create tag" closeLabel="Close" busy={state === "busy"}
      initialFocus={tagName} onClose={() => setOpened(false)}><form><label className="tag-manager-field">
        <span>Name</span><input ref={tagName} className="search-input" maxLength={80} readOnly={state === "busy"} defaultValue="Work" />
      </label></form></TagDialog>,
  };

  if (fixture in examples) {
    return <main className="showcase" data-fixture={fixture}>{examples[fixture as keyof typeof examples]}</main>;
  }
  return <main className="showcase">
    {Object.entries(examples).filter(([name]) => !["confirm", "tag-dialog", "toast", "progress"].includes(name))
      .map(([name, example]) => <section key={name} data-fixture={name}>{example}</section>)}
  </main>;
}

async function start() {
  for (const element of [document.documentElement, document.body]) element.classList.add("theme-" + theme, mode + "-mode");
  await loadBuiltinTheme(theme);
  document.body.dataset.appearance = JSON.stringify({ Mode: mode, Tokens: nativeUiPalette() });
  createRoot(document.getElementById("root")!).render(<Showcase />);
}

void start();
