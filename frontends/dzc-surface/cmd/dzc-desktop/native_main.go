package main

import (
	"context"
	"encoding/json"
	"log"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/model"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/uikit"
)

// All state below belongs to the main event thread. Reads stay on worker
// goroutines and carry a presentation context and a snapshot ticket.
type nativeMain struct {
	system  *SystemSettings
	winV    nativeWinVState
	window  *mygo.Window
	shell   *DesktopShell
	library *Library
	actions *History
	preview *Preview

	session               context.Context
	stop                  context.CancelFunc
	stopRead              context.CancelFunc
	searchTimer           *time.Timer
	resume                uint64
	history               nativeHistory
	cardMedia             nativeVisibleCache[nativeCardKey, nativeCardMedia]
	cardThumbnails        nativeCardThumbnails
	ocrPreview            *nativeOCRPreview
	ocrJob                nativeOCRJob
	ocrEngine             *protocol.OcrEngineInfo
	ocrEngineLoading      bool
	ocrEngineFailed       bool
	protectedRows         map[nativeCardKey]*nativeProtectedRow
	cardTagColors         map[string]string
	readingTagColors      bool
	tagColorRevision      uint64
	list                  uikit.HistoryListState
	historyScroll         ui.ScrollState
	searchWheel           float32
	towardsTop            time.Time
	preferences           map[string]string
	query, kind           string
	theme, mode           string
	importedThemes        []protocol.DesktopThemePackage
	importedTheme         *nativeTheme
	appearanceBusy        bool
	appearanceSync        bool
	appearanceMotionUntil time.Time
	appearanceRevision    uint64
	background            nativeBackground
	systemDark            bool
	windows11             bool
	focusSearch           bool
	temporarySearch       bool
	showSettings          bool
	showEmoji             bool
	emoji                 *nativeEmojiPanel
	settingsSection       string
	settingsOpen          map[string]*bool
	settingsScroll        ui.ScrollState
	settingsStackScroll   ui.ScrollState
	clearHistoryOpen      bool
	menu                  uikit.ContextMenu
	menuTransformsLoading bool
	readingViewSettings   bool
	viewSettingsRevision  uint64
	pendingSettings       map[string]bool
	cardMediaRevision     uint64
	resetOpen             bool
	privacyKindsOpen      bool
	privacyCustomOpen     bool
	focusPrivacyRules     bool
	sourceExclusions      nativePrivacyTextDraft
	customPrivacyRules    nativePrivacyTextDraft
	privacyTextTimer      *time.Timer
	privacyTextWrite      *nativePrivacyTextWrite
	privacyRules          nativePrivacyRules
	privacyDialog         *nativePrivacyDialog
	shortcut              *nativeShortcutForm
	savingShortcut        bool
	shortcutErrKey        string
	shortcutErrText       string
	queueCount            nativeQueueCount
	applications          nativeApplicationCatalog
	profile               nativeProfileSettings
	buildDetailsOpen      bool
	updater               nativeUpdater
	backup                *nativeBackupDialog
	surfaceOpacityDraft   *int
	fontChoices           *nativeFontChoices
	fontsFailed           bool
	readingFonts          bool
	capturing             bool
	captureError          string
	cli                   nativeCLISettings
	reading               bool
	private               bool
	privateUntil          *int64
	pinned                bool
	failed                bool
	mutating              bool
	notice                string
	keyboard              bool
	focusList             bool
	previewSelection      int64
	hover                 nativeHoverPreview
	tags                  *nativeTagEditor
	tagCatalog            nativeTagCatalog
	tagSuggestions        nativeTagSuggestions
	fileGesture           nativeFileGesture
	pinFocus              nativePinSource
	login                 nativeLoginState
	restartingAdmin       bool
	adminRestartFailed    bool
	tagLibrary            *nativeTagLibrary
	editor                *nativeEntryEditor
	transformKinds        []protocol.ToolTransformKind
	qr                    *nativeQRDialog
}

func (n *nativeMain) stopSession() {
	if n.stop != nil {
		n.stop()
		n.stop = nil
	}
	n.session = nil
	n.stopRead = nil
	n.cancelSearchTimer()
	n.history.reset(protocol.RuntimeState{}, nativeHistoryFilter{})
	n.cardMedia = nativeVisibleCache[nativeCardKey, nativeCardMedia]{}
	n.ocrJob, n.ocrEngine, n.ocrEngineLoading, n.ocrEngineFailed = nativeOCRJob{}, nil, false, false
	n.cardTagColors, n.readingTagColors = nil, false
	n.reading, n.failed, n.mutating, n.private = false, false, n.profile.moving || n.profile.restartPending || n.winV.changing || n.restartingAdmin, false
	n.privateUntil = nil
	n.notice, n.keyboard, n.focusList = "", false, false
	n.temporarySearch = false
	n.searchWheel, n.towardsTop = 0, time.Time{}
	n.capturing, n.captureError = false, ""
	n.profile.reading = false
	n.resetOpen = false
	n.clearHistoryOpen = false
	n.menuTransformsLoading, n.readingViewSettings = false, false
	n.pendingSettings = nil
	n.appearanceBusy, n.appearanceSync = false, false
	n.resetOpacityPreviews()
	n.background.pause()
	n.tags = nil
	n.closeEntryEditor()
	n.closeQR()
	n.closeBackup()
	n.closePrivacyDialog()
	n.cancelShortcut()
	n.savingShortcut = false
	n.shortcutErrKey, n.shortcutErrText = "", ""
	if !n.winV.changing {
		n.winV = nativeWinVState{}
	}
	n.queueCount = nativeQueueCount{}
	n.privacyRules = nativePrivacyRules{}
	if tags := n.tagLibrary; tags != nil {
		// Navigation outlives a hidden window, while confirmations, selection
		// and revealed contents are bound to the retired session.
		n.tagLibrary = &nativeTagLibrary{
			selected: tags.selected, query: tags.query, sort: tags.sort, nameQuery: tags.nameQuery, selectInitial: tags.selectInitial,
			railCollapsed: tags.railCollapsed,
		}
	}
	n.tagCatalog = nativeTagCatalog{}
	n.tagSuggestions = nativeTagSuggestions{}
	n.fileGesture = nativeFileGesture{}
	n.pinFocus = nativePinSource{}
	if n.login.pending {
		n.login = nativeLoginState{}
	}
	n.emoji = nil
	n.resetPreviewSelection()
}

func (n *nativeMain) prepare(request uint64) {
	if n.profile.moving || n.profile.restartPending {
		return
	}
	n.stopSession()
	session, cancel := context.WithCancel(context.Background())
	n.session, n.stop, n.resume = session, cancel, request
	n.reading = true
	go n.open(session)
}

func (n *nativeMain) open(session context.Context) {
	for session.Err() == nil {
		read, cancel := context.WithTimeout(session, 4*time.Second)
		status, err := n.library.Status(read)
		var settings map[string]string
		if err == nil {
			var raw json.RawMessage
			raw, err = n.library.preferences(read, status.State)
			if err == nil {
				err = json.Unmarshal(raw, &settings)
			}
		}
		if err == nil {
			cancel()
			n.update(session, func() {
				n.configured(settings)
				n.private = status.Privacy.Active
				n.privateUntil = status.Privacy.ExpiresAtMs
				n.history.reset(status.State, n.historyFilter())
				if n.private || !n.cardThumbnails.matches(status.State) {
					n.cardThumbnails = nativeCardThumbnails{}
				}
				if n.resume != 0 && n.shell.resumePosition {
					n.shell.resumePosition = false
					if settings["app.follow_mouse"] != "false" {
						placeWindowAtCursor(n.window)
					}
				}
				n.loadBackground()
				n.list.Reset()
				n.loadPage()
				go n.watch(session, status.State.EventSequence)
			})
			return
		}
		cancel()
		// A resident can wake the process while the Host is still opening
		// its profile. Keep the pending show request until the first page.
		timer := time.NewTimer(220 * time.Millisecond)
		select {
		case <-timer.C:
		case <-session.Done():
			timer.Stop()
			return
		}
	}
}

func (n *nativeMain) colorMode(settings map[string]string) string {
	if mode := settings["app.color_mode"]; mode == "light" || mode == "dark" {
		return mode
	}
	if n.systemDark {
		return "dark"
	}
	return "light"
}

func (n *nativeMain) configured(settings map[string]string) {
	themeName := settings["app.theme"]
	if themeName == "" {
		themeName = model.DefaultBuiltinTheme
	}
	mode := n.colorMode(settings)
	previous := n.preferences
	oldBase := n.themeBase()
	if previous == nil || themeName != n.theme || settings["app.custom_themes"] != previous["app.custom_themes"] {
		n.configureThemes(themeName, settings["app.custom_themes"])
	}
	appearanceChanged := themeName != n.theme || mode != n.mode ||
		(settings["app.show_app_border"] == "false") != (n.preferences["app.show_app_border"] == "false")
	language := mainLanguage(settings["app.language"])
	languageChanged := n.preferences == nil || n.language() != language
	n.preferences, n.theme, n.mode = settings, themeName, mode
	n.configureUpdater(settings[protocol.SettingUpdateChannel], settings[protocol.SettingUpdateAutoCheck] == "true")
	if n.window != nil && languageChanged {
		mygo.App.SetLocale(model.NativeLocale(string(language)))
	}
	if n.window != nil && (appearanceChanged || oldBase != n.themeBase()) {
		n.applyAppearance()
	}
	if settings["app.emoji_panel_enabled"] != "true" {
		n.showEmoji = false
	}
	if n.shell != nil && n.window != nil && (previous == nil || settings["app.edge_docking"] != previous["app.edge_docking"]) {
		n.shell.dock.configure(settings["app.edge_docking"] == "true")
	}
	n.syncWindowPin()
}

func (n *nativeMain) syncWindowPin() {
	pinned := n.preferences["app.window_pinned"] == "true" || n.shell != nil && n.shell.dock.autoPinned
	if n.pinned != pinned {
		n.pinned = pinned
		if n.window != nil {
			n.window.SetAlwaysOnTop(pinned)
		}
	}
}

func (n *nativeMain) systemAppearanceChanged() {
	dark := mygo.Theme.IsDark()
	if dark == n.systemDark {
		return
	}
	n.systemDark = dark
	if n.theme == "" {
		return
	}
	mode := n.colorMode(n.preferences)
	if mode != n.mode {
		n.mode = mode
		n.startAppearanceMotion()
		n.resetPreviewSelection()
	}
	// The OS can update the compositor material even with an explicit palette.
	n.applyAppearance()
	n.window.Invalidate()
}

func (n *nativeMain) applyAppearance() {
	if err := applyWindowAppearance(n.window, n.themeBase(), n.mode, n.preferences["app.show_app_border"] != "false"); err != nil {
		log.Printf("Native window appearance: %v", err)
	}
}

func (n *nativeMain) update(session context.Context, fn func()) {
	if session.Err() != nil {
		return
	}
	n.window.Update(func() {
		if !n.window.IsDestroyed() && session.Err() == nil && n.session == session {
			fn()
		}
	})
}

func (n *nativeMain) loadPage() {
	if n.session == nil || n.history.state.ServiceInstanceId == "" {
		return
	}
	n.cancelSearchTimer()
	if n.stopRead != nil {
		n.stopRead()
		n.stopRead = nil
	}
	if n.history.filter.sort != "" && n.history.filter.tag == "" {
		n.reading, n.failed = false, false
		n.revealWhenReady()
		return
	}
	request := n.history.request()
	session := n.session
	read, cancel := context.WithTimeout(session, 4*time.Second)
	n.stopRead, n.reading, n.failed = cancel, true, false
	go func() {
		defer cancel()
		page, err := readNativeHistoryRange(read, request, n.library.nativeHistoryPage)
		n.update(session, func() {
			if !n.history.accepts(request) {
				return
			}
			n.reading = false
			if err != nil || !n.history.apply(request, page) {
				n.failed = true
				if request.refreshLimit > 0 {
					n.history.clear()
				}
				if !n.window.IsVisible() && n.resume != 0 {
					n.retryPending(session, n.history.request())
				}
				return
			}
			n.revealWhenReady()
		})
	}()
}

func (n *nativeMain) retryPending(session context.Context, request nativeHistoryRequest) {
	time.AfterFunc(220*time.Millisecond, func() {
		n.update(session, func() {
			if n.resume != 0 && n.history.accepts(request) && n.failed {
				n.loadPage()
			}
		})
	})
}

func (n *nativeMain) reveal(session context.Context, request uint64, state protocol.RuntimeState) {
	ctx, cancel := context.WithTimeout(session, 4*time.Second)
	defer cancel()
	ok, _ := n.shell.completeResume(ctx, n.window, request, state.ServiceInstanceId, state.PrivacyEpoch)
	n.update(session, func() {
		if ok && n.resume == request {
			n.resume = 0
		}
	})
}

func (n *nativeMain) revealWhenReady() {
	if n.resume != 0 && !n.reading && !n.failed && !n.background.loading {
		go n.reveal(n.session, n.resume, n.history.state)
	}
}

func (n *nativeMain) watch(session context.Context, cursor uint64) {
	for session.Err() == nil {
		// Finish the bounded read when a view refresh retires its presentation.
		// Interrupting the transport here would also retire the original paste target.
		read, cancel := context.WithTimeout(context.WithoutCancel(session), 5*time.Second)
		events, err := n.actions.Changes(read, &cursor)
		cancel()
		if session.Err() != nil {
			return
		}
		if err != nil {
			n.update(session, func() { n.prepare(n.resume) })
			return
		}
		cursor = events.State.EventSequence
		if len(events.Events) > 0 || events.ResyncRequired {
			n.updateIfChanged(session, func() bool { return n.applyEvents(events) })
		}
	}
}

func (n *nativeMain) applyEvents(events protocol.EventsResult) bool {
	current, next := n.history.state, events.State
	if current.ServiceInstanceId != next.ServiceInstanceId || current.PrivacyEpoch != next.PrivacyEpoch || events.ResyncRequired {
		n.cardThumbnails = nativeCardThumbnails{}
		n.clearBackground()
		n.prepare(n.resume)
		return true
	}
	changed := false
	for _, event := range events.Events {
		if dialog := n.privacyDialog; dialog != nil && dialog.params.Id != nil &&
			(event.Event == protocol.EventKindHistoryReset ||
				(event.Event == protocol.EventKindEntryUpdated || event.Event == protocol.EventKindEntryDeleted) &&
					event.Data.EntryId != nil && *event.Data.EntryId == *dialog.params.Id) {
			n.closePrivacyDialog()
			changed = true
		}
		if event.Event == protocol.EventKindEntryUpdated && event.Data.EntryId != nil &&
			*event.Data.EntryId == n.ocrJob.id && n.ocrJob.status != "processing" {
			n.ocrJob = nativeOCRJob{}
			changed = true
		}
		if event.Event == protocol.EventKindHistoryReset {
			n.cardTagColors = nil
			n.tagColorRevision++
		}
		if event.Event == protocol.EventKindServiceStateChanged && event.Data.Status != nil {
			if n.queueCount.observe(*event.Data.Status) && n.settingEnabled("app.sequential_mode", false) {
				changed = true
			}
			switch *event.Data.Status {
			case "view_settings_changed", "capture_settings_changed", "custom_themes_changed", "PrivacyHotkeyChanged":
				if n.winV.changing {
					continue
				}
				if n.appearanceBusy {
					n.appearanceSync = true
					continue
				}
				n.refreshViewSettings()
			case "application_settings_reset":
				n.mutating, n.resetOpen = false, false
				n.resetOpacityPreviews()
				n.refreshViewSettings()
				changed = true
			}
		}
	}
	if next.HistoryRevision > current.HistoryRevision {
		n.cardThumbnails = nativeCardThumbnails{}
		affected, canReflow := historyRefreshEntries(current, events)
		removed := historyDeletionEntries(current.HistoryRevision, events.Events)
		deletionOnly := len(removed) > 0
		var tagReveal map[nativeCardKey]bool
		if canReflow && deletionOnly {
			n.resetPreviewInteraction()
		} else {
			if tags := n.tagLibrary; deletionOnly && tags != nil && tags.revealGeneration == n.history.generation {
				tagReveal = tags.reveal
			}
			n.resetPreviewSelection()
		}
		if canReflow {
			generation := n.history.generation
			n.history.applyPinEvents(events.Events)
			n.history.reflow(next, affected)
			if deletionOnly {
				n.retainTagViewAfterDeletion(generation, affected)
			}
		} else {
			n.history.reset(next, n.history.filter)
			if tagReveal != nil {
				// Fetch the remaining rows and authorize their details again
				// against the current epoch, keeping the user's tag view choice.
				for key := range tagReveal {
					if _, deleted := removed[key.id]; deleted {
						delete(tagReveal, key)
					}
				}
				n.tagLibrary.reveal, n.tagLibrary.revealGeneration = tagReveal, n.history.generation
			}
			if n.historyScroll.Y <= 0 {
				n.list.Reset()
			}
		}
		n.invalidateCardMedia(affected)
		if n.searchTimer == nil {
			n.loadPage()
		}
		n.tagCatalog.invalidate()
		changed = true
	}
	return changed
}

func (n *nativeMain) editQuery(composing bool) {
	filter := n.historyFilter()
	if n.session == nil || n.history.state.ServiceInstanceId == "" || composing || filter == n.history.filter {
		return
	}
	previous := n.history.filter
	n.resetPreviewSelection()
	n.history.reset(n.history.state, filter)
	n.list.Reset()
	n.cancelSearchTimer()
	if n.stopRead != nil {
		n.stopRead()
		n.stopRead = nil
	}
	// Type and sort changes take effect immediately. Text edits wait for the
	// same quiet period as WebView, and prior results cannot be activated.
	changingTagPane := n.tagLibrary != nil && (previous.sort != filter.sort || previous.tag != filter.tag)
	if changingTagPane || previous.kind != filter.kind || strings.TrimSpace(n.historyQuery()) == "" {
		n.loadPage()
		return
	}
	n.reading, n.failed = true, false
	session := n.session
	var timer *time.Timer
	timer = time.AfterFunc(300*time.Millisecond, func() {
		n.update(session, func() {
			if n.searchTimer == timer {
				n.loadPage()
			}
		})
	})
	n.searchTimer = timer
}

func (n *nativeMain) cancelSearchTimer() {
	if n.searchTimer != nil {
		n.searchTimer.Stop()
		n.searchTimer = nil
	}
}

func (n *nativeMain) historyFilter() nativeHistoryFilter {
	if tags := n.tagLibrary; tags != nil {
		query := strings.TrimSpace(tags.query)
		// The Host limits a tag-content search to 256 characters.
		if utf8.RuneCountInString(query) > 256 {
			query = string([]rune(query)[:256])
		}
		return nativeHistoryFilter{query: query, tag: tags.selected, sort: tags.sort}
	}
	query := strings.TrimSpace(n.query)
	if tag, ok := searchTagName(query); ok {
		return nativeHistoryFilter{tag: tag, kind: n.kind, sort: "pinned"}
	}
	return nativeHistoryFilter{query: query, kind: n.kind}
}

func searchTagName(query string) (string, bool) {
	prefix, tag, found := strings.Cut(strings.TrimSpace(query), ":")
	return strings.TrimSpace(tag), found && strings.EqualFold(prefix, "tag")
}

func (n *nativeMain) resetPreviewSelection() {
	n.clearProtectedRows()
	n.resetPreviewInteraction()
}

func (n *nativeMain) resetPreviewInteraction() {
	n.menu.Close()
	n.closeOCRPreview()
	n.keyboard = false
	n.previewSelection = 0
	n.hover.reset()
	n.closePreview()
}
