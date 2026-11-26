// dzc-desktop is the independent MyGo desktop entry point.
package main

import (
	"bufio"
	"context"
	_ "embed"
	"encoding/json"
	"fmt"
	"os"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/buildinfo"
)

//go:embed tray-icon.png
var trayIcon []byte

type History struct {
	actions       *client.Client
	events        *client.Client
	eventMu       sync.Mutex
	target        *client.Client
	targetMu      sync.Mutex
	targetInvalid atomic.Bool
	diagnostics   *desktopDiagnostics
}

type historyRow struct {
	ID      int64  `json:"id"`
	Preview string `json:"preview"`
	Kind    string `json:"kind"`
}
type historyPage struct {
	Rows  []historyRow          `json:"rows"`
	State protocol.RuntimeState `json:"state"`
	More  bool                  `json:"more"`
}

const pageSize = 40

func (h *History) List(ctx context.Context, query string, offset uint32) (historyPage, error) {
	request := protocol.DesktopPageParams{Offset: offset, Limit: pageSize + 1}
	if query != "" {
		request.Query = &query
	}
	var snapshot protocol.BodySnapshot
	host, err := h.actions.DesktopConnection(ctx)
	if err != nil {
		return historyPage{}, err
	}
	defer host.Close()
	if err := host.Call(ctx, protocol.MethodDesktopHistoryPage, request, "BodySnapshot", &snapshot); err != nil {
		return historyPage{}, err
	}
	var entries protocol.DesktopEntries
	if err := host.ReadSnapshot(ctx, snapshot, "DesktopEntries", &entries); err != nil {
		return historyPage{}, err
	}
	page := historyPage{Rows: []historyRow{}, More: len(entries.Entries) > pageSize, State: snapshot.State}
	for _, row := range entries.Entries[:min(pageSize, len(entries.Entries))] {
		page.Rows = append(page.Rows, historyRow{row.Id, row.Preview, row.ContentType})
	}
	return page, nil
}

func (h *History) Copy(ctx context.Context, id int64, state protocol.RuntimeState) error {
	var copied protocol.OperationResult
	err := h.actions.DesktopCall(ctx, protocol.MethodDesktopSurfaceCopy, protocol.DesktopSurfaceCopyParams{EntryId: id, State: state}, "OperationResult", &copied)
	if err == nil && !copied.Performed {
		return client.ErrProtocol
	}
	return err
}

func (h *History) Changes(ctx context.Context, cursor *uint64) (protocol.EventsResult, error) {
	wait := uint16(1000)
	var events protocol.EventsResult
	h.eventMu.Lock()
	defer h.eventMu.Unlock()
	var lastReadError error
	err := readEventStream(ctx, func(fresh bool) error {
		if fresh && h.events != nil {
			h.events.Close()
			h.events = nil
			h.targetInvalid.Store(true)
			h.diagnostics.record("events-reconnect", 0, lastReadError)
		}
		if h.events == nil {
			host, err := h.actions.DesktopConnection(ctx)
			if err != nil {
				return err
			}
			h.events = host
		}
		events = protocol.EventsResult{}
		if cursor == nil {
			// A new WebView loads current history and settings. Subscribe from that
			// point without replaying another window's edits into its first render.
			lastReadError = h.events.Call(ctx, protocol.MethodHealth, struct{}{}, "RuntimeState", &events.State)
			events.Events = []protocol.Event{}
			return lastReadError
		}
		lastReadError = h.events.Call(ctx, protocol.MethodEventsSubscribe, protocol.SubscribeParams{AfterSequence: *cursor, WaitMs: &wait}, "EventsResult", &events)
		return lastReadError
	})
	if err != nil {
		h.targetInvalid.Store(true)
		if h.events != nil {
			h.events.Close()
		}
		h.events = nil
	}
	h.diagnostics.eventRead(err)
	if err == nil {
		h.keepTargetAlive(ctx)
	}
	return events, err
}

// The target has its own authenticated stream: a long event poll must not
// delay presentation or paste. Event polling renews only a still-valid target.
func (h *History) keepTargetAlive(ctx context.Context) {
	if !h.targetMu.TryLock() {
		return
	}
	defer h.targetMu.Unlock()
	if h.target == nil || h.targetInvalid.Load() {
		return
	}
	var state protocol.RuntimeState
	if h.target.Call(ctx, protocol.MethodHealth, struct{}{}, "RuntimeState", &state) != nil {
		h.targetInvalid.Store(true)
		h.target.Close()
		h.target = nil
	}
}

func (h *History) rememberTarget(ctx context.Context) {
	h.targetMu.Lock()
	defer h.targetMu.Unlock()
	h.targetInvalid.Store(true)
	if h.target != nil {
		h.target.Close()
		h.target = nil
	}
	host, err := h.actions.DesktopConnection(ctx)
	if err != nil {
		return
	}
	h.target = host
	var result protocol.OperationResult
	// A desktop with no foreground editor can still display history and copy.
	allowPrevious := true
	if host.Call(ctx, protocol.MethodDesktopWindowTarget, protocol.WindowTargetParams{AllowPrevious: &allowPrevious}, "OperationResult", &result) == nil && result.Performed {
		h.targetInvalid.Store(false)
	}
}

func (h *History) CopyEntry(ctx context.Context, params protocol.DesktopCopyParams) (string, error) {
	if params.ServiceInstanceId != h.actions.Hello().State.ServiceInstanceId || params.Id == 0 {
		return "", client.ErrProtocol
	}
	if params.Paste {
		h.targetMu.Lock()
		defer h.targetMu.Unlock()
		if h.target == nil || h.targetInvalid.Swap(true) {
			// No paste was dispatched. Only this outcome may offer a new copy-only action.
			return "target_required", nil
		}
		if err := h.target.PasteWindowSelection(ctx, params); err != nil {
			fmt.Fprintf(os.Stderr, "Host window paste failed: %v\n", err)
			return "", err
		}
		return "completed", nil
	}
	var result protocol.OperationResult
	err := h.actions.DesktopCall(ctx, protocol.MethodDesktopClipboardCopy, params, "OperationResult", &result)
	if err == nil && !result.Performed {
		return "", client.ErrProtocol
	}
	return "completed", err
}

func (h *History) SendEmoji(ctx context.Context, kind, content string, epoch uint64, paste bool) (string, error) {
	params := protocol.WindowEmojiParams{ServiceInstanceId: h.actions.Hello().State.ServiceInstanceId,
		ExpectedEpoch: epoch, Kind: kind, Content: content, Paste: paste}
	if paste {
		h.targetMu.Lock()
		defer h.targetMu.Unlock()
		if h.target == nil || h.targetInvalid.Swap(true) {
			return "target_required", nil
		}
		if err := h.target.PasteWindowEmoji(ctx, params); err != nil {
			fmt.Fprintf(os.Stderr, "Host window emoji paste failed: %v\n", err)
			return "", err
		}
		return "completed", nil
	}
	var done protocol.OperationResult
	err := h.actions.DesktopCall(ctx, protocol.MethodDesktopWindowEmoji, params, "OperationResult", &done)
	if err != nil {
		return "", err
	}
	if !done.Performed {
		return "", client.ErrProtocol
	}
	return "completed", nil
}

func run() error {
	if len(os.Args) == 2 && os.Args[1] == profileRelaunchArg {
		if !acceptProfileRelaunch(os.Stdin) {
			return nil
		}
		os.Args[1] = "--show"
	}
	if handled, err := prepareElevation(); handled || err != nil {
		return err
	}
	if len(os.Args) == 2 && os.Args[1] == "--build-info" {
		return json.NewEncoder(os.Stdout).Encode(buildinfo.DesktopReport())
	}
	mainWindow := len(os.Args) == 3 && os.Args[1] == "--main-window"
	minimized := len(os.Args) == 2 && os.Args[1] == "--minimized"
	activation := ""
	if len(os.Args) == 2 {
		switch os.Args[1] {
		case "--show":
			activation = "show"
		case "--settings":
			activation = "settings"
		case "--search":
			activation = "search"
		}
	}
	if len(os.Args) != 1 && !mainWindow && !minimized && activation == "" {
		return fmt.Errorf("unexpected desktop arguments")
	}
	var updateWindowPipe *os.File
	if activation == "show" {
		updateWindowPipe = takeMyGoUpdateWindowPipe()
		if updateWindowPipe != nil {
			defer updateWindowPipe.Close()
		}
	}
	ctx := context.Background()
	bootStarted := time.Now()
	actions, err := client.OpenDesktop(ctx, buildinfo.Version, !mainWindow && elevatedInstance == "")
	if err != nil {
		return fmt.Errorf("open desktop Host: %w", err)
	}
	// Retain the authenticated endpoint identity only. Each action or visible
	// session opens its own stream, so idle panels need no keepalive requests.
	actions.Close()
	if elevatedInstance != "" && actions.Hello().State.ServiceInstanceId != elevatedInstance {
		return client.ErrProtocol
	}
	mygo.App.SetName("DezirClip MyGo")
	if !mainWindow {
		return runResident(actions, minimized, activation, updateWindowPipe)
	}
	if actions.Hello().State.ServiceInstanceId != os.Args[2] {
		return client.ErrProtocol
	}
	desktopTiming("child authentication", bootStarted)
	diagnostics := newDesktopDiagnostics(actions, "main")
	diagnostics.record("start", 0, nil)
	defer diagnostics.record("run-returned", 0, nil)
	history := &History{actions: actions, diagnostics: diagnostics}
	library := &Library{owner: actions}
	shell := &DesktopShell{diagnostics: diagnostics, resident: residentBridge{output: os.Stdout, grantFocus: grantResidentFocus}}
	preview := &Preview{owner: actions, resident: &shell.resident}
	systemSettings := &SystemSettings{owner: actions}
	defer systemSettings.close()
	settings := library.windowPreferences(ctx)
	follow := settings.FollowMouse != "false"
	savedSize := settings.savedSize()
	sizes := make(chan mygo.Size, 1)
	sizeDone := make(chan struct{})
	go library.saveWindowSizes(sizes, sizeDone)
	defer func() { close(sizes); <-sizeDone }()
	mygo.App.WhenReady(func() {
		windowStarted := time.Now()
		installMemoryPolicy(func() bool { return false })
		options := mygo.WindowOptions{Title: "DezirClip", Width: 352, Height: 380, MinWidth: 250, MinHeight: 300,
			Hidden: true, SkipTaskbar: true, DisableMaximize: true, Transparent: true, Vibrancy: mygo.VibrancyMica, TitleBarStyle: mygo.TitleBarHidden, HideWindowButtons: true}
		native := &nativeMain{
			shell: shell, library: library, actions: history, preview: preview, system: systemSettings,
			systemDark: mygo.Theme.IsDark(), windows11: shell.PlatformInfo().Windows11,
		}
		options.Content = ui.View(native.view)
		win := mygo.NewWindow(options)
		native.window = win
		shell.nativePrepare = native.prepare
		shell.keepMainAlive = func() bool { return native.updater.keepWindow() || native.winV.changing || native.restartingAdmin }
		shell.idleCloseDelay = func() (time.Duration, bool) {
			if native.mutating || native.readingViewSettings || native.history.state.ServiceInstanceId == "" {
				return 0, false
			}
			return idleDestroyDelay(native.preferences["app.idle_destroy_enabled"], native.preferences["app.idle_destroy_seconds"]), true
		}
		win.OnClosed(mygo.Theme.OnUpdated(native.systemAppearanceChanged))
		if err := win.SetIcon(trayIcon); err != nil {
			fmt.Fprintf(os.Stderr, "Window icon unavailable: %v\n", err)
		}
		preview.main = win
		dock := &shell.dock
		dock.window, dock.preview, dock.history = win, preview, history
		dock.show = func() { shell.requestShow(win); shell.resumeDocked = true }
		dock.configure(settings.EdgeDocking == "true")
		win.OnBlur(func() {
			preview.clear()
			native.menu.Close()
			concealed := native.clearProtectedRows()
			if native.editor != nil && native.editor.protected {
				native.closeEntryEditor()
				concealed = true
			}
			if native.qr != nil && native.qr.request.Reveal {
				native.closeQR()
				concealed = true
			}
			if native.backup != nil {
				concealed = native.backup.clearPreview() || concealed
			}
			if concealed {
				win.Invalidate()
			}
			focusChanged.Emit(win, false)
			// X11 passive shortcut grabs briefly blur GTK without switching apps.
			// Only an actual active-window loss cancels the native recording.
			if !mainWindowFocused(win) {
				if native.shortcut != nil {
					native.cancelShortcut()
					win.Invalidate()
				}
				history.targetInvalid.Store(true)
				fmt.Fprintln(os.Stdout, "recording-cancel")
			}
		})
		win.OnFocus(func() {
			if history.targetInvalid.Load() {
				history.rememberTarget(ctx)
			}
			fmt.Fprintln(os.Stdout, "focus")
			focusChanged.Emit(win, true)
		})
		win.OnShow(func() {
			fmt.Fprintln(os.Stdout, "visible")
			shell.idle.cancel()
			dock.hidden = false
			dock.shown = time.Now()
			dock.syncPolling()
			native.animateBackground()
			visibilityChanged.Emit(win, true)
		})
		win.OnHide(func() {
			dock.syncPolling()
			fmt.Fprintln(os.Stdout, "hidden")
			native.savePrivacyTexts(true)
			native.stopSession()
			preview.clear()
			visibilityChanged.Emit(win, false)
			fmt.Fprintln(os.Stdout, "recording-cancel")
		})
		win.OnMove(func() { preview.clear(); windowMoved.Emit(win, true) })
		win.OnFileDrop(func(e *mygo.FileDropEvent) {
			if fileDragPending.Load() {
				return
			}
			if native.showEmoji && !native.showSettings {
				native.importEmojiPaths(e.Paths)
			} else {
				native.dropFiles(e.Paths)
			}
		})
		win.OnClose(func(event *mygo.CloseEvent) {
			if native.winV.changing || native.updater.startingInstall {
				event.PreventDefault()
				shell.hideMain(win)
				return
			}
			diagnostics.record("window-close", 0, nil)
			fmt.Fprintln(os.Stdout, "closing")
		})
		win.OnClosed(func() {
			native.savePrivacyTexts(true)
			native.waitForPrivacyTexts()
			native.stopSession()
			native.stopUpdater()
			shell.idle.cancel()
			dock.hidden = false
			dock.configure(false)
			preview.clear()
			mygo.App.Quit()
		})
		if follow {
			placeWindowAtCursor(win)
		}
		if savedSize.Width != 0 {
			if scale := mainWindowScale(win); scale > 0 {
				size := scaledSize(savedSize.Width, savedSize.Height, 1/scale)
				win.SetContentSize(size.Width, size.Height)
				if follow {
					placeWindowAtCursor(win)
				} else {
					win.Center()
				}
			}
		}
		history.rememberTarget(ctx)
		diagnostics.record("native-content-ready", 0, nil)
		desktopTiming("child native window creation", windowStarted)
		shell.requestShow(win)
		physicalSize := func() mygo.Size {
			width, height := win.ContentSize()
			return scaledSize(width, height, mainWindowScale(win))
		}
		lastSize := physicalSize()
		observeSize := func() {
			if !win.IsVisible() || win.IsMinimized() || win.IsMaximized() || win.IsFullScreen() {
				return
			}
			size := physicalSize()
			if size == lastSize || size.Width < 200 || size.Height < 200 || size.Width > 32768 || size.Height > 32768 {
				return
			}
			lastSize = size
			select {
			case <-sizes:
			default:
			}
			sizes <- size
		}
		win.OnResize(func() { preview.clear(); observeSize() })
		win.OnClose(func(*mygo.CloseEvent) { observeSize() })
		go func() {
			var trayState trayToggleState
			commands := bufio.NewScanner(os.Stdin)
			commands.Buffer(nil, residentFrameLimit)
			for commands.Scan() {
				command := commands.Text()
				if len(command) > 0 && command[0] == '{' {
					if acceptRecordingEvent(win, command, native) {
						continue
					}
					reply := shell.resident.accept(command)
					if reply.UpdateStatus != nil || reply.UpdateChannel != "" {
						win.Update(func() {
							if reply.UpdateStatus != nil {
								native.updater.applyInstallStatus(*reply.UpdateStatus)
							}
							if reply.UpdateChannel != "" {
								native.updater.applyCheckEvent(reply)
							}
						})
					}
					continue
				}
				if command == "tray-toggle" {
					mygo.RunOnMain(func() {
						if win.IsDestroyed() {
							return
						}
						request, hide := trayState.toggle((win.IsVisible() && !win.IsMinimized()) || shell.resumePending.Load())
						if hide {
							shell.hideMain(win)
							fmt.Fprintln(os.Stdout, "tray-hide")
						} else {
							fmt.Fprintf(os.Stdout, "tray-show:%d\n", request)
						}
					})
					continue
				}
				if command == "tray-reset" {
					mygo.RunOnMain(trayState.cancel)
					continue
				}
				if value, found := strings.CutPrefix(command, "tray-show:"); found {
					request, err := strconv.ParseUint(value, 10, 64)
					if err != nil {
						continue
					}
					accepted := make(chan bool, 1)
					mygo.RunOnMain(func() { accepted <- trayState.accept(request) })
					if !<-accepted {
						continue
					}
					command = "show"
				} else if command == "show" || command == "toggle" || command == "settings" || command == "search" || command == "rebuild" {
					mygo.RunOnMain(trayState.cancel)
				}
				if command == "rebuild" {
					diagnostics.record("rebuild-request", 0, nil)
					mygo.RunOnMain(func() { win.Close() })
					continue
				}
				if command == "toggle" && mainWindowFocused(win) {
					mygo.RunOnMain(func() { shell.hideMain(win) })
					continue
				}
				if command == "show" || command == "toggle" || command == "settings" || command == "search" {
					wakeStarted := time.Now()
					// Accept the wake before any Host read can outlive the old idle
					// deadline. The UI-thread generation also invalidates a queued timer.
					wakeGeneration := shell.idle.acceptWake(mygo.RunOnMain)
					position := false
					if !mainWindowFocused(win) {
						preferences := make(chan windowPreferences, 1)
						go func() { preferences <- library.windowPreferences(ctx) }()
						history.rememberTarget(ctx)
						position = (<-preferences).FollowMouse != "false"
					}
					desktopTiming("warm native preparation", wakeStarted)
					mygo.RunOnMain(func() {
						if !win.IsDestroyed() && shell.idle.generation == wakeGeneration {
							if position {
								placeWindowAtCursor(win)
							}
							switch command {
							case "settings":
								native.openSettings()
							case "search":
								native.showSettings, native.showEmoji, native.focusSearch = false, false, true
								if native.tagLibrary != nil {
									native.closeTagLibrary()
								}
							}
							shell.requestShow(win)
						}
					})
				}
			}
			diagnostics.record("resident-pipe-ended", 0, commands.Err())
			mygo.App.Quit()
		}()
	})
	return mygo.App.Run()
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
