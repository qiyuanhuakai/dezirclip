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
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

// All state below belongs to the main event thread. Reads stay on worker
// goroutines and carry a presentation context and a snapshot ticket.
type nativeMain struct {
	window  *mygo.Window
	shell   *DesktopShell
	library *Library
	actions *History
	preview *Preview

	session          context.Context
	stop             context.CancelFunc
	stopRead         context.CancelFunc
	searchTimer      *time.Timer
	resume           uint64
	history          nativeHistory
	cardImages       nativeVisibleCache[nativeCardKey, ui.ImageSource]
	ocrPreview       *nativeOCRPreview
	ocrJob           nativeOCRJob
	ocrEngine        *protocol.OcrEngineInfo
	ocrEngineLoading bool
	protectedRows    map[nativeCardKey]*nativeProtectedRow
	cardTagColors    map[string]string
	readingTagColors bool
	tagColorRevision uint64
	list             uikit.HistoryListState
	historyScroll    ui.ScrollState
	searchWheel      float32
	towardsTop       time.Time
	preferences      map[string]string
	query, kind      string
	theme, mode      string
	importedThemes   []protocol.DesktopThemePackage
	importedTheme    *nativeTheme
	themeWorking     bool
	themeRefresh     bool
	systemDark       bool
	focusSearch      bool
	temporarySearch  bool
	showSettings     bool
	showEmoji        bool
	emoji            *nativeEmojiPanel
	settingsSection  string
	settingsOpen     map[string]*bool
	applications     nativeApplicationCatalog
	profile          nativeProfileSettings
	backup           *nativeBackupDialog
	fontChoices      *nativeFontChoices
	readingFonts     bool
	capturing        bool
	reading          bool
	private          bool
	pinned           bool
	failed           bool
	mutating         bool
	notice           string
	keyboard         bool
	focusList        bool
	previewSelection int64
	hover            nativeHoverPreview
	tags             *nativeTagEditor
	tagLibrary       *nativeTagLibrary
	editor           *nativeEntryEditor
	transform        *nativeTransformDialog
	transformKinds   []protocol.ToolTransformKind
	qr               *nativeQRDialog
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
	n.cardImages = nativeVisibleCache[nativeCardKey, ui.ImageSource]{}
	n.ocrJob, n.ocrEngine, n.ocrEngineLoading = nativeOCRJob{}, nil, false
	n.cardTagColors, n.readingTagColors = nil, false
	n.reading, n.failed, n.mutating, n.private = false, false, n.profile.moving || n.profile.restartPending, false
	n.notice, n.keyboard, n.focusList = "", false, false
	n.temporarySearch = false
	n.searchWheel, n.towardsTop = 0, time.Time{}
	n.capturing = false
	n.profile.reading = false
	n.themeWorking, n.themeRefresh = false, false
	n.tags = nil
	n.closeEntryEditor()
	n.closeTransform()
	n.closeQR()
	n.closeBackup()
	n.tagLibrary = nil
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
				n.history.reset(status.State, n.historyFilter())
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
		themeName = "mica"
	}
	mode := n.colorMode(settings)
	oldBase := n.themeBase()
	n.configureThemes(themeName, settings["app.custom_themes"])
	appearanceChanged := themeName != n.theme || mode != n.mode ||
		(settings["app.show_app_border"] == "false") != (n.preferences["app.show_app_border"] == "false")
	language := mainLanguage(settings["app.language"])
	languageChanged := n.preferences == nil || n.language() != language
	n.preferences, n.theme, n.mode = settings, themeName, mode
	if languageChanged {
		mygo.App.SetLocale(model.NativeLocale(string(language)))
	}
	if appearanceChanged || oldBase != n.themeBase() {
		n.applyAppearance()
	}
	if settings["app.emoji_panel_enabled"] != "true" {
		n.showEmoji = false
	}
	n.pinned = settings["app.window_pinned"] == "true"
	n.window.SetAlwaysOnTop(n.pinned)
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
		if n.resume != 0 {
			go n.reveal(n.session, n.resume, n.history.state)
		}
		return
	}
	request := n.history.request()
	session := n.session
	read, cancel := context.WithTimeout(session, 4*time.Second)
	n.stopRead, n.reading, n.failed = cancel, true, false
	go func() {
		defer cancel()
		page, err := n.library.nativeHistoryPage(read, request)
		n.update(session, func() {
			if !n.history.accepts(request) {
				return
			}
			n.reading = false
			if err != nil || !n.history.apply(request, page) {
				n.failed = true
				if !n.window.IsVisible() && n.resume != 0 {
					n.retryPending(session, request)
				}
				return
			}
			if n.resume != 0 {
				go n.reveal(session, n.resume, page.state)
			}
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

func (n *nativeMain) watch(session context.Context, cursor uint64) {
	for session.Err() == nil {
		read, cancel := context.WithTimeout(session, 5*time.Second)
		events, err := n.actions.Changes(read, &cursor)
		cancel()
		if err != nil {
			n.update(session, func() { n.prepare(n.resume) })
			return
		}
		cursor = events.State.EventSequence
		if len(events.Events) > 0 || events.ResyncRequired {
			n.update(session, func() { n.applyEvents(events) })
		}
	}
}

func (n *nativeMain) applyEvents(events protocol.EventsResult) {
	current, next := n.history.state, events.State
	if current.ServiceInstanceId != next.ServiceInstanceId || current.PrivacyEpoch != next.PrivacyEpoch || events.ResyncRequired {
		n.prepare(n.resume)
		return
	}
	for _, event := range events.Events {
		if event.Event == protocol.EventKindEntryUpdated && event.Data.EntryId != nil &&
			*event.Data.EntryId == n.ocrJob.id && n.ocrJob.status != "processing" {
			n.ocrJob = nativeOCRJob{}
		}
		if event.Event == protocol.EventKindHistoryReset {
			n.cardTagColors = nil
			n.tagColorRevision++
		}
		if event.Event == protocol.EventKindServiceStateChanged && event.Data.Status != nil {
			switch *event.Data.Status {
			case "view_settings_changed", "custom_themes_changed":
				if n.themeWorking {
					n.themeRefresh = true
					continue
				}
				n.prepare(n.resume)
				return
			case "application_settings_reset":
				n.prepare(n.resume)
				return
			}
		}
	}
	if next.HistoryRevision > current.HistoryRevision {
		n.resetPreviewSelection()
		n.history.reset(next, n.history.filter)
		if n.historyScroll.Y <= 0 {
			n.list.Reset()
		}
		if n.searchTimer == nil {
			n.loadPage()
		}
		n.loadTagCatalog()
	}
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
		return nativeHistoryFilter{tag: tag, sort: "time"}
	}
	return nativeHistoryFilter{query: query, kind: n.kind}
}

func searchTagName(query string) (string, bool) {
	prefix, tag, found := strings.Cut(strings.TrimSpace(query), ":")
	return strings.TrimSpace(tag), found && strings.EqualFold(prefix, "tag")
}

func (n *nativeMain) resetPreviewSelection() {
	n.clearProtectedRows()
	n.closeOCRPreview()
	n.keyboard = false
	n.previewSelection = 0
	n.hover.reset()
	n.closePreview()
}

func (n *nativeMain) saveSetting(key, value string) {
	n.writeSetting(func(ctx context.Context, epoch uint64) error {
		return n.library.SetSetting(ctx, key, value, epoch)
	})
}

func (n *nativeMain) togglePin() {
	pinned := !n.pinned
	n.writeSetting(func(ctx context.Context, epoch uint64) error {
		return n.library.SetWindowPinned(ctx, pinned, epoch)
	})
}

func (n *nativeMain) writeSetting(write func(context.Context, uint64) error) {
	if n.session == nil || n.mutating || n.history.state.ServiceInstanceId == "" {
		return
	}
	n.mutating, n.notice = true, ""
	session, state := n.session, n.history.state
	go func() {
		ctx, cancel := context.WithTimeout(session, 4*time.Second)
		defer cancel()
		err := write(ctx, state.PrivacyEpoch)
		n.update(session, func() {
			n.mutating = false
			if err != nil {
				n.notice = n.text("native_setting_save_failed")
				return
			}
			n.prepare(n.resume)
		})
	}()
}
