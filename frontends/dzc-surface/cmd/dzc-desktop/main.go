// dzc-desktop is the independent MyGo desktop entry point.
package main

import (
	"context"
	"embed"
	"encoding/json"
	"fmt"
	"io/fs"
	"log"
	"os"

	"github.com/egoist/mygo"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/buildinfo"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/surfaces/quickpaste"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/surfaces/screenshot"
)

//go:embed web
var web embed.FS

//go:embed tray-icon.png
var trayIcon []byte

type History struct {
	actions *client.Client
	events  *client.Client
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
	if err := h.actions.Call(ctx, protocol.MethodDesktopHistoryPage, request, "BodySnapshot", &snapshot); err != nil {
		return historyPage{}, err
	}
	var entries protocol.DesktopEntries
	if err := h.actions.ReadSnapshot(ctx, snapshot, "DesktopEntries", &entries); err != nil {
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
	err := h.actions.Call(ctx, protocol.MethodDesktopSurfaceCopy, protocol.DesktopSurfaceCopyParams{EntryId: id, State: state}, "OperationResult", &copied)
	if err == nil && !copied.Performed {
		return client.ErrProtocol
	}
	return err
}

func (h *History) Changes(ctx context.Context, cursor uint64) (protocol.EventsResult, error) {
	wait := uint16(1000)
	var events protocol.EventsResult
	err := h.events.Call(ctx, protocol.MethodEventsSubscribe, protocol.SubscribeParams{AfterSequence: cursor, WaitMs: &wait}, "EventsResult", &events)
	return events, err
}

func run() error {
	if len(os.Args) == 2 && os.Args[1] == "--build-info" {
		return json.NewEncoder(os.Stdout).Encode(buildinfo.DesktopReport())
	}
	if len(os.Args) != 1 {
		return fmt.Errorf("unexpected desktop arguments")
	}
	// The Desktop always loads the packaged local UI even in a developer shell.
	os.Unsetenv("MYGO_DEV_URL")
	ctx := context.Background()
	actions, err := client.OpenDesktop(ctx, buildinfo.Version, true)
	if err != nil {
		return fmt.Errorf("open desktop Host: %w", err)
	}
	defer actions.Close()
	events, err := client.OpenDesktop(ctx, buildinfo.Version, false)
	if err != nil {
		return err
	}
	defer events.Close()
	if actions.Hello().State.ServiceInstanceId != events.Hello().State.ServiceInstanceId {
		return client.ErrProtocol
	}
	assets, err := fs.Sub(web, "web")
	if err != nil {
		return err
	}
	mygo.SetFrontend(assets)
	mygo.App.SetName("DezirClip MyGo")
	mygo.Bind(&History{actions: actions, events: events})
	preview := &Preview{owner: actions}
	mygo.Bind(preview)
	mygo.App.WhenReady(func() {
		panel := quickpaste.NewDesktopPanel(actions)
		selector := screenshot.New(actions)
		tray, err := mygo.NewTray(mygo.TrayOptions{Icon: trayIcon, ToolTip: "DezirClip"})
		open := func() {
			if win := preview.main; win != nil {
				win.Show()
				win.Focus()
				return
			}
			win := mygo.NewWindow(mygo.WindowOptions{Title: "DezirClip", Width: 660, Height: 620})
			preview.main, preview.generation = win, 0
			win.OnBlur(preview.clear)
			win.OnFocus(panel.RefreshShortcut)
			win.OnFocus(selector.Refresh)
			win.OnClosed(func() {
				preview.clear()
				preview.main = nil
				if tray == nil {
					mygo.App.Quit()
				}
			})
			win.Page().OnWillNavigate(func(e *mygo.NavigateEvent) { e.PreventDefault() })
			win.Page().SetWindowOpenHandler(func(mygo.WindowOpenRequest) *mygo.WindowOptions { return nil })
			win.Page().LoadURL("mygo://localhost/")
		}
		if err != nil {
			log.Printf("Tray unavailable: %v", err)
		} else {
			tray.OnClick(open)
			tray.SetMenu(mygo.NewMenu([]*mygo.MenuItem{
				{Label: "打开剪贴板", Click: func(*mygo.MenuItem, *mygo.Window) { open() }},
				{Label: "退出", Role: mygo.RoleQuit},
			}))
			mygo.App.OnWillQuit(func(*mygo.QuitEvent) { tray.Destroy() })
		}
		open()
	})
	return mygo.App.Run()
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
