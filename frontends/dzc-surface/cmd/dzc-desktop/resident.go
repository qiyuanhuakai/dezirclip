package main

import (
	"bufio"
	"fmt"
	"io"
	"log"
	"os"
	"os/exec"

	"github.com/egoist/mygo"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/surfaces/quickpaste"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/surfaces/screenshot"
)

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
		open := func() {
			if commands != nil {
				if _, err := io.WriteString(commands, "show\n"); err != nil {
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
					if scanner.Text() == "focus" {
						mygo.RunOnMain(func() {
							if !stopping {
								panel.RefreshShortcut()
								selector.Refresh()
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
					if !stopping && tray == nil {
						mygo.App.Quit()
					}
				})
			}()
		}
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
