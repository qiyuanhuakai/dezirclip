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
func refreshMainShortcuts(owner *client.Client, registered map[string]bool, toggle func(), record func(string) bool) map[string]string {
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
			if err := mygo.GlobalShortcut.Register(key, func() {
				if !record(key) {
					toggle()
				}
			}); err != nil {
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
		mainPID := 0
		recorder := &shortcutRecording{post: mygo.RunOnMain}
		panel.SetShortcutRecorder(recorder.offer)
		selector.SetShortcutRecorder(recorder.offer)
		gpuDisabled := false
		stopping := false
		closing := false
		migrating := false
		moved := false
		var pending *client.MoveRecovery
		reopen := ""
		registered := make(map[string]bool)
		searchKeys := make(map[string]bool)
		pasteKeys := map[string]map[string]bool{"app.sequential_hotkey": {}, "app.rich_paste_hotkey": {}}
		pasteBusy := false
		registerPaste := func(setting, key string) error {
			return mygo.GlobalShortcut.Register(key, func() {
				if recorder.offer(key) || pasteBusy || stopping || migrating || moved || pending != nil {
					return
				}
				work, err := prepareShortcutPaste(owner, setting == "app.rich_paste_hotkey")
				if err != nil {
					log.Printf("Paste shortcut target unavailable: %v", err)
					return
				}
				pasteBusy = true
				go func() {
					if err := work(); err != nil {
						log.Printf("Paste shortcut failed: %v", err)
					}
					mygo.RunOnMain(func() { pasteBusy = false })
				}()
			})
		}
		refreshPaste := func(settings map[string]string) {
			if settings == nil {
				return
			}
			for setting, fallback := range map[string]string{"app.sequential_hotkey": "Alt+V", "app.rich_paste_hotkey": "Ctrl+Shift+Z"} {
				key, found := settings[setting]
				if !found {
					key = fallback
				}
				if err := changeMainHotkeys(pasteKeys[setting], key, func(key string) error { return registerPaste(setting, key) }, mygo.GlobalShortcut.Unregister, func() error { return nil }); err != nil {
					log.Printf("Paste shortcut unavailable: %v", err)
				}
			}
		}
		reachable := func() bool {
			return len(registered) > 0 || len(searchKeys) > 0 || (tray != nil && trayHostAvailable(tray))
		}
		var toggle func()
		var present func(string)
		registerSearch := func(key string) error {
			return mygo.GlobalShortcut.Register(key, func() {
				if !recorder.offer(key) {
					present("search")
				}
			})
		}
		refreshSearch := func(settings map[string]string) {
			if settings == nil {
				return
			}
			key, found := settings["app.search_hotkey"]
			if !found {
				key = "Alt+F"
			}
			if err := changeMainHotkeys(searchKeys, key, registerSearch, mygo.GlobalShortcut.Unregister, func() error { return nil }); err != nil {
				log.Printf("Search shortcut unavailable: %v", err)
			}
		}
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
				if err := grantMainFocus(mainPID); err != nil {
					log.Printf("Main activation permission unavailable: %v", err)
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
			mainPID = child.Process.Pid
			if err := grantMainFocus(mainPID); err != nil {
				log.Printf("New main activation permission unavailable: %v", err)
			}
			if command == "settings" || command == "search" {
				if _, err := io.WriteString(input, command+"\n"); err != nil {
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
									response := residentReply{ID: request.ID, Success: err == nil}
									if err != nil {
										response.Error = err.Error()
										if request.Operation == "recording" {
											log.Printf("Shortcut recording transition failed (enabled=%t): %v", !request.Disabled, err)
										}
									}
									_ = json.NewEncoder(input).Encode(response)
								}
								if stopping || migrating || commands != input || ((moved || pending != nil) && request.Operation != "relaunch-profile") {
									reply(fmt.Errorf("main window closed"))
									return
								}
								switch request.Operation {
								case "restart-admin":
									if pasteBusy {
										reply(fmt.Errorf("PasteBusy"))
										break
									}
									database, err := owner.DesktopDatabase()
									if err != nil {
										reply(err)
										break
									}
									if err := recorder.stop(); err != nil {
										reply(err)
										break
									}
									migrating = true
									go func() {
										err := elevateDesktop(owner.Hello().State.ServiceInstanceId, database)
										mygo.RunOnMain(func() {
											migrating = false
											reply(err)
											if err == nil {
												mygo.App.Quit()
											}
										})
									}()
								case "reset-settings":
									if pasteBusy {
										reply(fmt.Errorf("PasteBusy"))
										break
									}
									if err := recorder.stop(); err != nil {
										reply(err)
										break
									}
									if err := resetSettings(owner, request.Epoch); err != nil {
										reply(err)
										break
									}
									relaunch, closing = true, true
									reply(nil)
									_, _ = io.WriteString(input, "rebuild\n")
								case "paste-hotkey":
									keys, ok := pasteKeys[request.Panel]
									if !ok {
										reply(fmt.Errorf("InvalidActionHotkey"))
										break
									}
									reply(changeMainHotkeys(keys, request.Hotkey, func(key string) error { return registerPaste(request.Panel, key) }, mygo.GlobalShortcut.Unregister, func() error {
										return persistActionHotkey(owner, request.Panel, request.Hotkey, request.Epoch)
									}))
								case "search-hotkey":
									reply(changeMainHotkeys(searchKeys, request.Hotkey, registerSearch, mygo.GlobalShortcut.Unregister, func() error {
										return persistActionHotkey(owner, "app.search_hotkey", request.Hotkey, request.Epoch)
									}))
								case "panel-hotkey":
									fallback := map[string]string{"app.quick_paste_hotkey": "Ctrl+Shift+V", "app.screenshot_hotkey": "Ctrl+Shift+A", "app.screenshot_full_hotkey": "Ctrl+Shift+F"}
									previous, ok := fallback[request.Panel]
									if !ok {
										reply(fmt.Errorf("InvalidPanelHotkey"))
										return
									}
									ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
									settings, err := (&Library{owner}).Preferences(ctx)
									cancel()
									var values map[string]string
									if err == nil {
										err = json.Unmarshal(settings, &values)
									}
									if err != nil {
										reply(err)
										return
									}
									if saved, found := values[request.Panel]; found {
										previous = saved
									}
									refresh := selector.Refresh
									if request.Panel == "app.quick_paste_hotkey" {
										refresh = panel.RefreshShortcut
									}
									reply(changeNativePanel(previous, request.Hotkey, func(value string) error {
										return persistPanelHotkey(owner, request.Panel, value, request.Epoch)
									}, refresh))
								case "main-hotkeys":
									reply(changeMainHotkeys(registered, request.Hotkey, func(key string) error {
										return mygo.GlobalShortcut.Register(key, func() {
											if !recorder.offer(key) {
												toggle()
											}
										})
									}, mygo.GlobalShortcut.Unregister, func() error { return persistMainHotkeys(owner, request.Hotkey, request.Epoch) }))
								case "test-hotkey":
									err := testNativeHotkey(request.Hotkey)
									if err != nil {
										log.Printf("Shortcut availability check failed: %v", err)
									}
									reply(err)
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
								case "recording":
									if request.Disabled {
										reply(recorder.stop())
										return
									}
									reply(recorder.start(owner, request.Epoch, func(event recordingEvent) {
										if commands == input {
											_ = json.NewEncoder(input).Encode(event)
										}
									}))
								case "native-panel":
									key := ""
									switch request.Panel {
									case "quick-paste":
										key = "app.quick_paste_enabled"
									case "screenshot":
										key = "app.screenshot_enabled"
									default:
										reply(fmt.Errorf("invalid native panel"))
										return
									}
									ctx, cancel := context.WithTimeout(context.Background(), 4*time.Second)
									library := &Library{owner}
									preferences, err := library.Preferences(ctx)
									var values map[string]string
									if err == nil {
										err = json.Unmarshal(preferences, &values)
									}
									if err == nil {
										refresh := panel.RefreshShortcut
										if request.Panel == "screenshot" {
											refresh = selector.Refresh
										}
										err = changeNativePanel(values[key] != "false", !request.Disabled, func(enabled bool) error {
											saveCtx, stop := context.WithTimeout(context.Background(), 2*time.Second)
											defer stop()
											return library.SetSetting(saveCtx, key, strconv.FormatBool(enabled), request.Epoch)
										}, refresh)
									}
									cancel()
									reply(err)
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
					case "recording-cancel":
						mygo.RunOnMain(func() { recorder.abort() })
					case "closing":
						mygo.RunOnMain(func() { closing = true })
					case "focus":
						mygo.RunOnMain(func() {
							if !stopping && !migrating && !moved && pending == nil {
								panel.RefreshShortcut()
								selector.Refresh()
								settings := refreshMainShortcuts(owner, registered, toggle, recorder.offer)
								refreshSearch(settings)
								refreshPaste(settings)
							}
						})
					}
				}
				if err := child.Wait(); err != nil {
					log.Printf("Main window exited: %v", err)
				}
				mygo.RunOnMain(func() {
					_ = recorder.stop()
					input.Close()
					commands = nil
					mainPID = 0
					closing = false
					if moved || relaunch {
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
		startupSettings := refreshMainShortcuts(owner, registered, toggle, recorder.offer)
		refreshSearch(startupSettings)
		refreshPaste(startupSettings)
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
			_ = recorder.stop()
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
