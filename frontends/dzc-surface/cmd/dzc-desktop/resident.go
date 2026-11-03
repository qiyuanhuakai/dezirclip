package main

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"os"
	"os/exec"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/surfaces/quickpaste"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/surfaces/screenshot"
)

// The resident keeps the configured keyboard shortcuts after the WebView exits.
func refreshMainShortcuts(owner *client.Client, registered map[string]bool, toggle func()) {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	settings, err := (&Library{owner}).Preferences(ctx)
	var values map[string]string
	if err != nil || json.Unmarshal(settings, &values) != nil {
		log.Print("Main window shortcut settings unavailable")
		return
	}
	wanted := make(map[string]bool)
	for _, line := range strings.FieldsFunc(values["app.hotkey"], func(r rune) bool { return r == '\r' || r == '\n' }) {
		key := strings.TrimSpace(line)
		if key != "" && !strings.EqualFold(key, "MouseMiddle") && !strings.EqualFold(key, "MButton") {
			wanted[key] = true
		}
	}
	for key := range registered {
		if !wanted[key] {
			mygo.GlobalShortcut.Unregister(key)
			delete(registered, key)
		}
	}
	for key := range wanted {
		if !registered[key] {
			if err := mygo.GlobalShortcut.Register(key, toggle); err != nil {
				log.Printf("Main window shortcut unavailable: %v", err)
			} else {
				registered[key] = true
			}
		}
	}
}

// Only the main window loads the WebView runtime. Parent and child use
// inherited pipes for window lifecycle, and authenticate separately to Rust.
func runResident(owner *client.Client) error {
	executable, err := os.Executable()
	if err != nil {
		return err
	}
	mygo.App.WhenReady(func() {
		panel := quickpaste.NewDesktopPanel(owner)
		selector := screenshot.New(owner)
		tray, err := mygo.NewTray(mygo.TrayOptions{Icon: trayIcon, ToolTip: "DezirClip"})
		if err != nil {
			log.Printf("Tray unavailable: %v", err)
		}
		var commands io.WriteCloser
		stopping := false
		closing := false
		reopen := false
		registered := make(map[string]bool)
		var toggle func()
		var present func(string)
		present = func(command string) {
			if commands != nil {
				if closing {
					reopen = true
					return
				}
				if _, err := io.WriteString(commands, command+"\n"); err != nil {
					reopen = true
					log.Printf("Main window unavailable: %v", err)
				}
				return
			}
			child := exec.Command(executable, "--main-window", owner.Hello().State.ServiceInstanceId)
			child.Stderr = os.Stderr
			input, err := child.StdinPipe()
			if err != nil {
				log.Print(err)
				return
			}
			events, err := child.StdoutPipe()
			if err != nil {
				input.Close()
				log.Print(err)
				return
			}
			if err := child.Start(); err != nil {
				input.Close()
				events.Close()
				log.Print(err)
				return
			}
			commands = input
			go func() {
				scanner := bufio.NewScanner(events)
				for scanner.Scan() {
					line := scanner.Text()
					if strings.HasPrefix(line, "{") {
						var request captureRequest
						if json.Unmarshal([]byte(line), &request) == nil && request.ID != 0 {
							mygo.RunOnMain(func() {
								selector.Capture(request.Full, request.Epoch, func(err error) {
									_ = json.NewEncoder(input).Encode(captureReply{ID: request.ID, Success: err == nil})
								})
							})
						}
						continue
					}
					switch line {
					case "closing":
						mygo.RunOnMain(func() { closing = true })
					case "focus":
						mygo.RunOnMain(func() {
							if !stopping {
								panel.RefreshShortcut()
								selector.Refresh()
								refreshMainShortcuts(owner, registered, toggle)
							}
						})
					}
				}
				if err := child.Wait(); err != nil {
					log.Printf("Main window exited: %v", err)
				}
				mygo.RunOnMain(func() {
					input.Close()
					commands = nil
					closing = false
					if !stopping && reopen {
						reopen = false
						present("show")
					} else if !stopping && tray == nil && len(registered) == 0 {
						mygo.App.Quit()
					}
				})
			}()
		}
		toggle = func() { present("toggle") }
		open := func() { present("show") }
		refreshMainShortcuts(owner, registered, toggle)
		if tray != nil {
			tray.OnClick(open)
			tray.SetMenu(mygo.NewMenu([]*mygo.MenuItem{
				{Label: "打开剪贴板", Click: func(*mygo.MenuItem, *mygo.Window) { open() }},
				{Label: "退出", Role: mygo.RoleQuit},
			}))
			mygo.App.OnWillQuit(func(*mygo.QuitEvent) { tray.Destroy() })
		}
		mygo.App.OnBeforeQuit(func(*mygo.QuitEvent) {
			stopping = true
			for key := range registered {
				mygo.GlobalShortcut.Unregister(key)
			}
			if commands != nil {
				commands.Close()
			}
		})
		open()
	})
	if err := mygo.App.Run(); err != nil {
		return fmt.Errorf("native desktop: %w", err)
	}
	return nil
}
