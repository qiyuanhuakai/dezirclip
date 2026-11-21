import { useRef, useState, type CSSProperties } from "react";
import "../src/styles/components/privacy.css";
import { Pin, Tag, X } from "lucide-react";
import { createRoot } from "react-dom/client";
import { AppHeader, CharacterPanel, CharacterSearch, ClipboardSearch, ConfirmDialog, EmojiGrid, EmojiTabs, FilterChips, HistoryCard, HistoryCardAction, HistoryCardContent, HistoryCardMeta, HistoryCardTag, HistoryCardTags, HotkeyBinding, MatchHighlight,
  ProgressToast, QuickPastePanel, QuickPasteRow, QuickPasteSearch, ScrollToTopButton, SearchBar, SearchHistory, SettingHelp, SettingsDisclosure, SettingsFooter, SettingsRail, SettingsSectionCard,
  SettingsSlider, SettingsToggle, SettingsToggleField, TagDialog, TagHistoryEntry, ThemedSelect, ToastContainer, VirtualClipboardList, type EmojiTab, type SearchMode } from "../src";
import { isBuiltinTheme, loadBuiltinTheme } from "../src/themes";
import { nativeUiPalette } from "../src/native-appearance";
import { BackgroundSettingsFixture, ThemeManagerFixture } from "./appearance-fixtures";
import { CaptureActionsFixture, CaptureEngineFixture, FixedCaptureRendererFixture } from "./capture-fixtures";
import { TagSuggestion, TagSuggestions, TagSuggestionRetry, TagSuggestionStatus } from "../src/components/TagSuggestions";
import { CLISettingsFixture } from "./cli-fixture";
import { AttachmentFixture, HistoryImageFixture, HistoryOCRFixture, HistoryRichTextFixture, PreviewImageFixture, PreviewToolbarFixture, ResizeFixture, WheelFixture } from "./preview-fixtures";
import { BackupDialogFixture, ContextMenuFixture, EmojiFavoritesFixture, HotkeyListFixture, SettingsChoicesFixture, SettingsUpdateFixture,
  PrivacyDetectionFixture, PrivacyRulesFixture, SettingsDataFixture, SettingsFieldsFixture, SettingsRemoveFixture, SettingsSectionFixture, SettingsSectionGridFixture, SearchSelectFixture, TagBatchToolbarFixture, TagSearchFixture, TagToolbarFixture, TextFieldsFixture, UpdateActionsFixture } from "./form-menu-fixtures";
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

function OptionalShortcutFixture() {
  const [shortcut, setShortcut] = useState("Ctrl+Alt+P");
  return <HotkeyBinding label="Privacy session shortcut" value={shortcut} placeholder="Not set" unset
    recordingLabel="Press a shortcut" recording={false} busy={false} disabled={false}
    helpLabel="Setting help" description="Leave empty to keep this shortcut disabled." clearLabel="Clear shortcut"
    onClear={() => setShortcut("")} onClick={undefined} onKeyDown={undefined} onMouseDown={undefined} onBlur={undefined} />;
}

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

const suggestionTags = [
  { tag: "Design", color: "#ed7841" },
  { tag: "Documents", color: "#687ee2" },
  { tag: "Work", color: "#25a58b" },
];

function TagSuggestionsFixture() {
  const [selected, setSelected] = useState(state === "active" ? "Documents" : "");
  const [query, setQuery] = useState(selected ? "tag:" + selected : "");
  const input = useRef<HTMLInputElement>(null);
  const tags = state === "empty" ? [] : suggestionTags;
  return <div className="showcase-tag-suggestions">
    <ClipboardSearch t={() => "Search clipboard"} contentTypes={[]} search={query} setSearch={setQuery} isComposing={false}
      setIsComposing={() => undefined} searchInputRef={input} showTagFilter setShowTagFilter={() => undefined}
      searchIsFocused setSearchIsFocused={() => undefined} setEditingTagsId={() => undefined}
      typeFilter={null} setTypeFilter={() => undefined} renderTagFilter={({ id }) =>
        <TagSuggestions label="Tags" count={tags.length}>
          {state === "empty" && <TagSuggestionRetry>Retry loading tags</TagSuggestionRetry>}
          {tags.length === 0 ? <TagSuggestionStatus id={id} role="listbox" aria-label="Tags">No matching tags</TagSuggestionStatus>
            : <div id={id} role="listbox" aria-label="Tags">
              {tags.map(({ tag, color }) => <TagSuggestion key={tag} tag={tag} color={color} active={tag === selected}
                onClick={() => { setSelected(tag); setQuery("tag:" + tag); }} />)}
            </div>}
        </TagSuggestions>}
    />
  </div>;
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
  ...(state === "long-label" ? [{ id: "applications", labelKey: "Default Applications and File Associations" }] : []),
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
        <HistoryCardContent>{row.summary}</HistoryCardContent>
      </HistoryCard>} />
  </div>;
}

function HistoryTypographyFixture() {
  const compact = state.startsWith("compact");
  const custom = state === "custom";
  const size = state === "small" ? 11 : state.endsWith("large") ? 18 : 13;
  const style = {
    width: 336, display: "flex", flexDirection: "column", "--clipboard-item-font-size": `${size}px`,
    ...(custom ? { "--font-main": '"Georgia"', "--font-mono": '"Courier New"' } : {}),
  } as CSSProperties;
  return <div style={style}>
    {([
      ["short", "Font sample: Wm12 中文"],
      ["wrapped", "A clipboard preview can wrap naturally at the card edge. ".repeat(8).trim()],
      ["concealed", "Sensitive content hidden"],
    ] as const).map(([name, text]) => <div key={name} style={{ paddingBottom: compact ? 2 : 4 }}>
      <HistoryCard compact={compact} disableLayout data-card-metrics={name}>
      <HistoryCardMeta source="Editor" time="12:30" actions={<>
        <HistoryCardAction title="Pin"><Pin size={12} /></HistoryCardAction>
        <HistoryCardAction title="Tags"><Tag size={12} /></HistoryCardAction>
        <HistoryCardAction title="Delete"><X size={12} /></HistoryCardAction>
      </>} />
      <HistoryCardContent concealed={name === "concealed"}>{text}</HistoryCardContent>
      </HistoryCard>
    </div>)}
  </div>;
}

function TagHistoryEntriesFixture() {
  const [selected, setSelected] = useState([false, true]);
  const managing = state !== "browse";
  return <div style={{ width: state === "compact" ? 245 : 360 }}>
    {["Project notes", "Release checklist"].map((name, index) => <TagHistoryEntry key={name} aria-label={name}
      selection={managing ? { label: "Select entry: " + name, checked: selected[index],
        onToggle: () => setSelected(row => row.map((checked, slot) => slot === index ? !checked : checked)) } : undefined}>
      <HistoryCard compact={state === "compact"} selected={managing && selected[index]} disableLayout>
        <HistoryCardMeta source="Editor" time="12:30" actions={<>
          <HistoryCardAction title="Pin"><Pin size={12} /></HistoryCardAction>
          <HistoryCardAction title="Tags"><Tag size={12} /></HistoryCardAction>
          <HistoryCardAction title="Delete"><X size={12} /></HistoryCardAction>
        </>} />
        <HistoryCardContent>{name}</HistoryCardContent>
      </HistoryCard>
    </TagHistoryEntry>)}
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
  const [enabled, setEnabled] = useState(state === "enabled" || fixture === "settings-toggle-help");
  const [opacity, setOpacity] = useState(72);
  const [tab, setTab] = useState<EmojiTab>(state === "favorites" ? "favorites" : "emoji");

  const examples = {
    "text-fields": <TextFieldsFixture />,
    "settings-field": <SettingsFieldsFixture />,
    "context-menu": <ContextMenuFixture state={state} />,
    "settings-section-card": state === "wide" ? <SettingsSectionGridFixture /> : <SettingsSectionFixture open={state === "expanded"} />,
    "settings-choices": <SettingsChoicesFixture />,
    "hotkey-list": <HotkeyListFixture multiple={state === "multiple" || state === "win-v"} extended={state === "extended"} winV={state === "win-v"} />,
    "emoji-favorites": <EmojiFavoritesFixture />,
    "settings-data": <SettingsDataFixture />,
    "theme-manager": <ThemeManagerFixture state={state} />,
    "background-settings": <BackgroundSettingsFixture state={state} />,
    "capture-actions": <CaptureActionsFixture state={state} />,
    "capture-engine": <CaptureEngineFixture state={state} />,
    "fixed-capture-renderer": <FixedCaptureRendererFixture />,
    "cli-settings": <CLISettingsFixture state={state} />,
    "backup-dialog": <BackupDialogFixture state={state} />,
    "settings-update": <SettingsUpdateFixture state={state} />,
    "settings-update-actions": <UpdateActionsFixture state={state} />,
    "settings-remove": <SettingsRemoveFixture />,
    "privacy-rules": <PrivacyRulesFixture resized={state === "resized"} />,
    "privacy-detection": <PrivacyDetectionFixture narrow={state === "narrow" || state === "resized"} />,
    "tag-history-entry": <TagHistoryEntriesFixture />,
    "tag-toolbar": <TagToolbarFixture />,
    "tag-batch-toolbar": <TagBatchToolbarFixture state={state} />,
    "tag-search": <TagSearchFixture />,
    "tag-suggestions": <TagSuggestionsFixture />,
    "search-select": <SearchSelectFixture state={state} />,
    "attachment-preview": <AttachmentFixture state={state} />,
    "history-image": <HistoryImageFixture state={state} />,
    "history-ocr": <HistoryOCRFixture state={state} />,
    "preview-toolbar": <PreviewToolbarFixture state={state} />,
    "preview-image": <PreviewImageFixture state={state} />,
    "preview-resize-handles": <ResizeFixture />,
    "wheel-hot-zone": <WheelFixture />,
    "history-card-meta": <HistoryCard disableLayout><HistoryCardMeta source="Editor" time="12:30"
      sourceIcon={<Pin size={10} />} actions={<button type="button" className="btn-icon" title="Tags"><Tag size={12} /></button>} /></HistoryCard>,
    "history-list": <HistoryListFixture />,
    "history-typography": <HistoryTypographyFixture />,
    "history-rich-text": <HistoryRichTextFixture state={state} />,
    "settings-rail": <SettingsRail t={key => key} groups={settingsGroups} activeGroup={activeSettings} onSelect={setActiveSettings} />,
    "settings-footer": <SettingsFooter t={key => footerLabels[key] ?? key} productName="DezirClip 0.2"
      compact={state === "compact"} onFeedback={() => undefined} onWebsite={() => undefined} onResetSettings={() => undefined} />,
    "settings-toggle": <div className="setting-item"><span className="item-label">Keep clipboard history</span>
      <SettingsToggle aria-label="Keep clipboard history" checked={enabled} onChange={event => setEnabled(event.currentTarget.checked)} />
    </div>,
    "settings-keyboard-preview": <div className="showcase-fields settings-view showcase-toggle-help">
      <SettingsToggleField label="Arrow key selection" helpLabel="Setting help" description="Use arrow keys to select clipboard entries."
        checked={enabled} onChange={event => setEnabled(event.target.checked)} />
      <SettingsToggleField detail label="Preview keyboard selection" helpLabel="Setting help"
        description="Show the selected item's preview when using arrow keys." defaultChecked={state === "enabled"} disabled={!enabled} />
    </div>,
    "settings-toggle-help": <div className="showcase-fields settings-view showcase-toggle-help">
      <SettingsToggleField label="Move pasted entries to top" helpLabel="Setting help"
        description="After pasting, move the item to the top of the list." checked={enabled}
        onChange={event => setEnabled(event.currentTarget.checked)} />
      <SettingsToggleField label="Use Win+V" helpLabel="Setting help"
        description="Use Win+V to open DezirClip." checked={false} disabled />
    </div>,
    "settings-slider": <div className="settings-view showcase-settings"><div className="setting-item column">
      <span className="item-label">Opacity {opacity}%</span>
      <SettingsSlider min={0} max={100} value={opacity} aria-label="Opacity" onChange={event => setOpacity(event.currentTarget.valueAsNumber)} />
    </div></div>,
    "hotkey-binding": <HotkeyBinding label="Quick Paste" value="Ctrl+Shift+V" placeholder="Set shortcut" recordingLabel="Press a shortcut"
      recording={state === "recording"} busy={false} disabled={false} onClick={undefined} onKeyDown={undefined}
      onMouseDown={undefined} onBlur={undefined} />,
    "hotkey-optional": <OptionalShortcutFixture />,
    "scroll-to-top": <div className="showcase-scroll-top">Clipboard history<ScrollToTopButton label="Scroll to top" onClick={() => undefined} /></div>,
    "character-search": <CharacterSearch label="Search symbols" value={query} onChange={setQuery} />,
    "emoji-tabs": <EmojiTabs active={tab} labels={{ emoji: "Emoji", favorites: "Favorites" }} onChange={setTab} />,
    "emoji-grid": <EmojiGrid groups={[{ name: "Frequently used", emojis: ["😀", "😄", "😂", "🤣", "😊", "😍", "😘", "😎", "🤔", "🔥", "✨", "👌"] }]}
      empty="No emoji" onSelect={() => undefined} />,
    "history-tags": <HistoryCard compact={state.startsWith("compact")} disableLayout
      style={{ "--clipboard-tag-font-size": state === "large" ? "15px" : "10px" } as CSSProperties}>
      <HistoryCardMeta source="Editor" time="12:30" actions={<>
        <HistoryCardAction title="Pin"><Pin size={12} /></HistoryCardAction>
        <HistoryCardAction title="Tags"><Tag size={12} /></HistoryCardAction>
        <HistoryCardAction title="Delete"><X size={12} /></HistoryCardAction>
      </>} />
      <HistoryCardContent>Saved from Editor</HistoryCardContent>
      <HistoryCardTags>
        <HistoryCardTag color="#245ed6">Work</HistoryCardTag>
        <HistoryCardTag color="#7c377c">Reference</HistoryCardTag>
      </HistoryCardTags>
    </HistoryCard>,
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
      <HistoryCardContent>Copied from the active editor</HistoryCardContent>
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
    {Object.entries(examples).filter(([name]) => !["backup-dialog", "confirm", "tag-dialog", "toast", "progress"].includes(name))
      .map(([name, example]) => <section key={name} data-fixture={name}>{example}</section>)}
  </main>;
}

function SurfaceOpacityFixture() {
  return <main className="showcase main-content" data-fixture="surface-opacity">
    <div className="showcase-opacity-columns">
      <SettingsSectionCard title="Appearance" collapsed={false} onToggle={() => undefined}>
        <span className="item-label">Color theme</span>
        <ThemedSelect ariaLabel="Color mode" options={options} value="system" onChange={() => undefined} searchable />
      </SettingsSectionCard>
      <HistoryCard disableLayout>
        <HistoryCardMeta source="Editor" time="12:30" actions={<>
          <HistoryCardAction title="Pin"><Pin size={12} /></HistoryCardAction>
          <HistoryCardAction title="Tags"><Tag size={12} /></HistoryCardAction>
          <HistoryCardAction title="Delete"><X size={12} /></HistoryCardAction>
        </>} />
        <HistoryCardContent>Clipboard entry</HistoryCardContent>
      </HistoryCard>
    </div>
  </main>;
}

const symbolRange = (first: number, count: number) => Array.from({ length: count }, (_, index) => String.fromCodePoint(first + index));
const symbolGroups = [
  { name: "Arrows", emojis: symbolRange(0x2190, 112) },
  { name: "Technical symbols", emojis: symbolRange(0x2300, 104) },
  { name: "Geometric shapes", emojis: symbolRange(0x25a0, 80) },
  { name: "Mathematics", emojis: ["∑", "∞", "√", "∫", "≈", "≠", "≤", "≥"] },
];

function CharacterPanelFixture() {
  const [shown, setShown] = useState(2);
  const loadMore = useRef(() => setShown(count => count + 1));
  return <main className="showcase showcase-character-panel" data-fixture="character-panel">
    <CharacterPanel>
      <EmojiTabs active="symbols" labels={{ emoji: "Emoji", symbols: "Symbols", favorites: "Favorites" }} onChange={() => undefined} />
      <CharacterSearch label="Search symbols" value="" onChange={() => undefined} />
      <EmojiGrid groups={symbolGroups.slice(0, shown)} empty="No results" onSelect={() => undefined}
        onLoadMore={shown < symbolGroups.length ? loadMore.current : undefined} />
    </CharacterPanel>
  </main>;
}

function QuickPasteFixture() {
  return <main data-fixture="quick-paste-panel">
    <QuickPastePanel>
      <QuickPasteSearch aria-label="Search clipboard" placeholder="Search clipboard…" />
      <div className="quick-paste-window__list">
        <QuickPasteRow preview="Release checklist" source="Editor" active />
        <QuickPasteRow preview="Design notes" source="Browser" />
        <QuickPasteRow preview="Preview package ready for testing" source="Terminal" />
      </div>
    </QuickPastePanel>
  </main>;
}

async function start() {
  for (const element of [document.documentElement, document.body]) element.classList.add("theme-" + theme, mode + "-mode");
  await loadBuiltinTheme(theme);
  if (fixture === "surface-opacity" || fixture === "quick-paste-panel") {
    document.documentElement.classList.add("showcase-opacity-background");
    document.documentElement.style.setProperty("--surface-opacity", String(Math.min(100, Math.max(0, Number(state))) / 100));
  }
  if (fixture === "quick-paste-panel") document.body.classList.add("quick-paste");
  document.body.dataset.appearance = JSON.stringify({ Mode: mode, Tokens: nativeUiPalette() });
  createRoot(document.getElementById("root")!).render(fixture === "surface-opacity" ? <SurfaceOpacityFixture />
    : fixture === "character-panel" ? <CharacterPanelFixture />
    : fixture === "quick-paste-panel" ? <QuickPasteFixture /> : <Showcase />);
}

void start();
