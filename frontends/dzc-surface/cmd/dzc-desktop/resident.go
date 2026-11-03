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
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/surfaces/quickpaste"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/surfaces/screenshot"
)

// The resident keeps the configured keyboard shortcuts after the WebView exits.
func refreshMainShortcuts(owner *client.Client, registered map[string]bool, toggle func()) map[string]string {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	settings, err := (&Library{owner}).Preferences(ctx)
	var values map[string]string
	if err != nil || json.Unmarshal(settings, &values) != nil {
		log.Print("Main window shortcut settings unavailable")
		return nil
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
	return values
}

// Only the main window loads the WebView runtime. Parent and child use
// inherited pipes for window lifecycle, and authenticate separately to Rust.
func runResident(owner *client.Client, minimized, forceShow bool) error {
	executable, err := os.Executable()
	if err != nil {
		return err
	}
	relaunch := false
	mygo.App.WhenReady(func() {
		panel := quickpaste.NewDesktopPanel(owner)
		selector := screenshot.New(owner)
		var tray *mygo.Tray
		var setTrayVisible func(bool) error
		pinnedCount := 0
		var commands io.WriteCloser
		gpuDisabled := false
		stopping := false
		closing := false
		migrating := false
		moved := false
		var pending *client.MoveRecovery
		reopen := ""
		registered := make(map[string]bool)
		reachable := func() bool {
			return len(registered) > 0 || (tray != nil && trayHostAvailable(tray))
		}
		var toggle func()
		var present func(string)
		reconnect := func(report func(error)) {
			migrating = true
			receipt := pending
			go func() {
				ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
				defer cancel()
				err := receipt.Reconnect(ctx)
				mygo.RunOnMain(func() {
					migrating = false
					if err == nil {
						pending = nil
						moved = true
					}
					if report != nil {
						report(err)
					}
					if err != nil {
						if report == nil || commands == nil {
							_, _ = mygo.Dialog.Message(mygo.MessageOptions{Title: "DezirClip", Message: "Data moved. Open DezirClip again to retry the Service connection.\n" + err.Error(), Buttons: []string{"OK"}})
						}
						return
					}
					relaunch = true
					if commands == nil {
						mygo.App.Quit()
						return
					}
					closing = true
					_, _ = io.WriteString(commands, "rebuild\n")
				})
			}()
		}
		present = func(command string) {
			if stopping || migrating || moved {
				return
			}
			if pending != nil {
				reconnect(nil)
				return
			}
			if commands != nil {
				if closing {
					if reopen != "settings" {
						reopen = command
					}
					return
				}
				if _, err := io.WriteString(commands, command+"\n"); err != nil {
					if reopen != "settings" {
						reopen = command
					}
					log.Printf("Main window unavailable: %v", err)
				}
				return
			}
			child := exec.Command(executable, "--main-window", owner.Hello().State.ServiceInstanceId)
			if saved := (&Library{owner}).windowPreferences(context.Background()).GpuDisabled; saved != "" {
				gpuDisabled = saved == "true"
			}
			if gpuDisabled {
				configureSoftwareWebview(child)
			}
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
			if command == "settings" {
				if _, err := io.WriteString(input, "settings\n"); err != nil {
					log.Printf("Main window unavailable: %v", err)
					reopen = command
				}
			}
			go func() {
				scanner := bufio.NewScanner(events)
				for scanner.Scan() {
					line := scanner.Text()
					if strings.HasPrefix(line, "{") {
						var request residentRequest
						if json.Unmarshal([]byte(line), &request) == nil && request.ID != 0 {
							mygo.RunOnMain(func() {
								reply := func(err error) {
									_ = json.NewEncoder(input).Encode(residentReply{ID: request.ID, Success: err == nil})
								}
								if stopping || migrating || commands != input || ((moved || pending != nil) && request.Operation != "relaunch-profile") {
									reply(fmt.Errorf("main window closed"))
									return
								}
								switch request.Operation {
								case "move-profile":
									migrating = true
									go func() {
										receipt, err := client.MoveActiveProfile(context.Background(), request.Directory, owner.Hello().State.ServiceInstanceId, request.Epoch)
										mygo.RunOnMain(func() {
											migrating = false
											pending = receipt
											moved = err == nil && pending == nil
											if commands == input {
												result := residentReply{ID: request.ID, Success: err == nil, Reconnect: pending != nil}
												if err != nil {
													result.Error = err.Error()
												}
												_ = json.NewEncoder(input).Encode(result)
											} else if moved {
												relaunch = true
												mygo.App.Quit()
											}
										})
									}()
								case "relaunch-profile":
									if pending != nil {
										reconnect(reply)
										return
									}
									if !moved {
										reply(fmt.Errorf("no completed profile move"))
										return
									}
									relaunch, closing = true, true
									reply(nil)
									_, _ = io.WriteString(input, "rebuild\n")
								case "pin-preview":
									err := openPinnedPreview(owner, request.Preview, func() {
										pinnedCount--
										if pinnedCount == 0 && commands == nil && !migrating && !moved && pending == nil && !reachable() && !panel.HasShortcut() && !selector.HasShortcut() {
											mygo.App.Quit()
										}
									})
									if err == nil {
										pinnedCount++
									}
									reply(err)
								case "capture":
									selector.Capture(request.Full, request.Epoch, reply)
								case "webview-gpu":
									ctx, cancel := context.WithTimeout(context.Background(), 4*time.Second)
									err := (&Library{owner}).SetSetting(ctx, "app.disable_webview_gpu", strconv.FormatBool(request.Disabled), request.Epoch)
									cancel()
									if err != nil || request.Disabled == gpuDisabled {
										reply(err)
										return
									}
									gpuDisabled = request.Disabled
									closing, reopen = true, "settings"
									reply(nil)
									if _, err := io.WriteString(input, "rebuild\n"); err != nil {
										log.Printf("Closing WebView for GPU change: %v", err)
									}
								case "tray":
									wasVisible := tray != nil
									if err := setTrayVisible(request.Visible); err != nil {
										reply(err)
										return
									}
									ctx, cancel := context.WithTimeout(context.Background(), 4*time.Second)
									err := (&Library{owner}).SetSetting(ctx, "app.hide_tray_icon", strconv.FormatBool(!request.Visible), request.Epoch)
									cancel()
									if err != nil {
										if restoreErr := setTrayVisible(wasVisible); restoreErr != nil {
											log.Printf("Restoring tray: %v", restoreErr)
										}
									}
									reply(err)
								default:
									reply(fmt.Errorf("unknown resident request"))
								}
							})
						}
						continue
					}
					switch line {
					case "closing":
						mygo.RunOnMain(func() { closing = true })
					case "focus":
						mygo.RunOnMain(func() {
							if !stopping && !migrating && !moved && pending == nil {
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
					if moved {
						relaunch = true
						mygo.App.Quit()
					} else if migrating || pending != nil {
						return
					} else if !stopping && reopen != "" {
						command := reopen
						reopen = ""
						present(command)
					} else if !stopping && pinnedCount == 0 && !panel.HasShortcut() && !selector.HasShortcut() && !reachable() {
						mygo.App.Quit()
					}
				})
			}()
		}
		toggle = func() { present("toggle") }
		open := func() { present("show") }
		startupSettings := refreshMainShortcuts(owner, registered, toggle)
		setTrayVisible = func(visible bool) error {
			if (tray != nil) == visible {
				return nil
			}
			if !visible {
				tray.Destroy()
				tray = nil
				return nil
			}
			created, err := mygo.NewTray(mygo.TrayOptions{Icon: trayIcon, ToolTip: "DezirClip"})
			if err != nil {
				return err
			}
			tray = created
			tray.OnClick(open)
			tray.SetMenu(mygo.NewMenu([]*mygo.MenuItem{
				{Label: "打开剪贴板", Click: func(*mygo.MenuItem, *mygo.Window) { open() }},
				{Label: "设置", Click: func(*mygo.MenuItem, *mygo.Window) { present("settings") }},
				{Label: "退出", Role: mygo.RoleQuit},
			}))
			return nil
		}
		if err := setTrayVisible(startupSettings["app.hide_tray_icon"] != "true"); err != nil {
			log.Printf("Tray unavailable: %v", err)
		}
		mygo.App.OnWillQuit(func(*mygo.QuitEvent) {
			if tray != nil {
				tray.Destroy()
			}
		})
		mygo.App.OnBeforeQuit(func(event *mygo.QuitEvent) {
			if migrating {
				event.PreventDefault()
				return
			}
			stopping = true
			for key := range registered {
				mygo.GlobalShortcut.Unregister(key)
			}
			if commands != nil {
				commands.Close()
			}
		})
		silent := startupSettings != nil && startupSettings["app.silent_start"] != "false"
		if forceShow || (!minimized && !silent) || !reachable() {
			open()
		}
	})
	if err := mygo.App.Run(); err != nil {
		return fmt.Errorf("native desktop: %w", err)
	}
	if relaunch {
		child := exec.Command(executable, "--show")
		child.Stderr = os.Stderr
		if err := child.Start(); err != nil {
			return err
		}
		return child.Process.Release()
	}
	return nil
}
