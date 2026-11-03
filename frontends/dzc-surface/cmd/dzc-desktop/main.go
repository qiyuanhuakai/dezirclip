// dzc-desktop is the independent MyGo desktop entry point.
package main

import (
	"bufio"
	"context"
	"crypto/sha256"
	"embed"
	"encoding/json"
	"fmt"
	"io/fs"
	"os"
	"sync"
	"sync/atomic"

	"github.com/egoist/mygo"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/buildinfo"
)

//go:embed web
var web embed.FS

//go:embed tray-icon.png
var trayIcon []byte

type History struct {
	actions       *client.Client
	events        *client.Client
	eventMu       sync.Mutex
	targetInvalid atomic.Bool
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
	if h.events == nil {
		h.targetInvalid.Store(true)
		host, err := h.actions.DesktopConnection(ctx)
		if err != nil {
			return events, err
		}
		h.events = host
	}
	var err error
	if cursor == nil {
		// A new WebView loads current history and settings. Subscribe from that
		// point without replaying another window's edits into its first render.
		err = h.events.Call(ctx, protocol.MethodHealth, struct{}{}, "RuntimeState", &events.State)
		events.Events = []protocol.Event{}
	} else {
		err = h.events.Call(ctx, protocol.MethodEventsSubscribe, protocol.SubscribeParams{AfterSequence: *cursor, WaitMs: &wait}, "EventsResult", &events)
	}
	if err != nil {
		h.targetInvalid.Store(true)
		h.events.Close()
		h.events = nil
	}
	return events, err
}

// Capture before showing the WebView. Its subscription keeps this exact
// authenticated session alive while the user searches or edits their selection.
func (h *History) rememberTarget(ctx context.Context) {
	h.eventMu.Lock()
	defer h.eventMu.Unlock()
	h.targetInvalid.Store(true)
	if h.events != nil {
		h.events.Close()
		h.events = nil
	}
	host, err := h.actions.DesktopConnection(ctx)
	if err != nil {
		return
	}
	h.events = host
	var result protocol.OperationResult
	// A desktop with no foreground editor can still display history and copy.
	if host.Call(ctx, protocol.MethodDesktopWindowTarget, struct{}{}, "OperationResult", &result) == nil && result.Performed {
		h.targetInvalid.Store(false)
	}
}

func (h *History) CopyEntry(ctx context.Context, params protocol.DesktopCopyParams) (string, error) {
	if params.ServiceInstanceId != h.actions.Hello().State.ServiceInstanceId || params.Id == 0 {
		return "", client.ErrProtocol
	}
	if params.Paste {
		h.eventMu.Lock()
		defer h.eventMu.Unlock()
		if h.events == nil || h.targetInvalid.Swap(true) {
			// No paste was dispatched. Only this outcome may offer a new copy-only action.
			return "target_required", nil
		}
		if err := h.events.PasteWindowSelection(ctx, params); err != nil {
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
		h.eventMu.Lock()
		defer h.eventMu.Unlock()
		if h.events == nil || h.targetInvalid.Swap(true) {
			return "target_required", nil
		}
		if err := h.events.PasteWindowEmoji(ctx, params); err != nil {
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
	if len(os.Args) == 2 && os.Args[1] == "--build-info" {
		return json.NewEncoder(os.Stdout).Encode(buildinfo.DesktopReport())
	}
	assets, err := fs.Sub(web, "web")
	if err != nil {
		return err
	}
	if len(os.Args) == 2 && os.Args[1] == "--web-asset-hashes" {
		hashes := map[string]string{}
		err := fs.WalkDir(assets, ".", func(path string, entry fs.DirEntry, err error) error {
			if err != nil || entry.IsDir() {
				return err
			}
			data, err := fs.ReadFile(assets, path)
			if err == nil {
				hashes[path] = fmt.Sprintf("%x", sha256.Sum256(data))
			}
			return err
		})
		if err != nil {
			return err
		}
		return json.NewEncoder(os.Stdout).Encode(hashes)
	}
	mainWindow := len(os.Args) == 3 && os.Args[1] == "--main-window"
	minimized := len(os.Args) == 2 && os.Args[1] == "--minimized"
	forceShow := len(os.Args) == 2 && os.Args[1] == "--show"
	if len(os.Args) != 1 && !mainWindow && !minimized && !forceShow {
		return fmt.Errorf("unexpected desktop arguments")
	}
	// The Desktop always loads the packaged local UI even in a developer shell.
	os.Unsetenv("MYGO_DEV_URL")
	ctx := context.Background()
	actions, err := client.OpenDesktop(ctx, buildinfo.Version, !mainWindow)
	if err != nil {
		return fmt.Errorf("open desktop Host: %w", err)
	}
	// Retain the authenticated endpoint identity only. Each action or visible
	// session opens its own stream, so idle panels need no keepalive requests.
	actions.Close()
	mygo.App.SetName("DezirClip MyGo")
	if !mainWindow {
		return runResident(actions, minimized, forceShow)
	}
	if actions.Hello().State.ServiceInstanceId != os.Args[2] {
		return client.ErrProtocol
	}
	mygo.SetFrontend(assets)
	history := &History{actions: actions}
	history.rememberTarget(ctx)
	mygo.Bind(history)
	library := &Library{owner: actions}
	mygo.Bind(library)
	shell := &DesktopShell{resident: residentBridge{output: os.Stdout, grantFocus: grantResidentFocus}}
	mygo.Bind(shell)
	preview := &Preview{owner: actions}
	mygo.Bind(preview)
	settings := library.windowPreferences(ctx)
	follow := settings.FollowMouse != "false"
	savedSize := settings.savedSize()
	sizes := make(chan mygo.Size, 1)
	sizeDone := make(chan struct{})
	go library.saveWindowSizes(sizes, sizeDone)
	defer func() { close(sizes); <-sizeDone }()
	mygo.App.WhenReady(func() {
		win := mygo.NewWindow(mygo.WindowOptions{Title: "DezirClip", Width: 352, Height: 380, MinWidth: 250, MinHeight: 300,
			Hidden: true, Transparent: true, Vibrancy: mygo.VibrancyMica, TitleBarStyle: mygo.TitleBarHidden, HideWindowButtons: true})
		preview.main = win
		win.OnBlur(func() { history.targetInvalid.Store(true); preview.clear(); focusChanged.Emit(win, false) })
		win.OnFocus(func() { fmt.Fprintln(os.Stdout, "focus"); focusChanged.Emit(win, true) })
		win.OnShow(func() { visibilityChanged.Emit(win, true) })
		win.OnHide(func() { preview.clear(); visibilityChanged.Emit(win, false) })
		win.OnMove(func() { windowMoved.Emit(win, true) })
		win.OnFileDrop(func(e *mygo.FileDropEvent) { filesDropped.Emit(win, e.Paths) })
		win.OnClose(func(*mygo.CloseEvent) { fmt.Fprintln(os.Stdout, "closing") })
		win.OnClosed(func() { preview.clear(); mygo.App.Quit() })
		win.Page().OnWillNavigate(func(e *mygo.NavigateEvent) { e.PreventDefault() })
		win.Page().SetWindowOpenHandler(func(mygo.WindowOpenRequest) *mygo.WindowOptions { return nil })
		win.Page().LoadURL("mygo://localhost/")
		if follow {
			placeMainAtCursor(win)
		}
		if savedSize.Width != 0 {
			if scale := mainWindowScale(win); scale > 0 {
				size := scaledSize(savedSize.Width, savedSize.Height, 1/scale)
				win.SetContentSize(size.Width, size.Height)
				if follow {
					placeMainAtCursor(win)
				} else {
					win.Center()
				}
			}
		}
		win.Show()
		win.Focus()
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
		win.OnResize(observeSize)
		win.OnClose(func(*mygo.CloseEvent) { observeSize() })
		go func() {
			commands := bufio.NewScanner(os.Stdin)
			for commands.Scan() {
				command := commands.Text()
				if len(command) > 0 && command[0] == '{' {
					shell.resident.accept(command)
					continue
				}
				if command == "toggle" && win.IsFocused() {
					mygo.RunOnMain(func() { win.Close() })
					continue
				}
				if command == "show" || command == "toggle" || command == "settings" {
					position := false
					if !win.IsFocused() {
						history.rememberTarget(ctx)
						position = library.windowPreferences(ctx).FollowMouse != "false"
					}
					mygo.RunOnMain(func() {
						if !win.IsDestroyed() {
							if position {
								placeMainAtCursor(win)
							}
							win.Show()
							win.Focus()
							if command == "settings" {
								shell.settingsPending.Store(true)
								settingsRequested.Emit(win, true)
							}
						}
					})
				}
			}
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
