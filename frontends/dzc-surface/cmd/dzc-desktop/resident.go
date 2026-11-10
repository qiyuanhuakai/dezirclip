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
		key := mainShortcutName(line)
		if key != "" {
			wanted[key] = true
		}
	}
	for key := range registered {
		if !wanted[key] {
			unregisterMainShortcut(key)
			delete(registered, key)
		}
	}
	for key := range wanted {
		if !registered[key] {
			if err := registerMainShortcut(key, func() {
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
func runResident(owner *client.Client, minimized bool, activation string) error {
	executable, err := os.Executable()
	if err != nil {
		return err
	}
	relaunch := false
	relaunchCommand := "show"
	var mainChild *exec.Cmd
	var mainDone <-chan struct{}
	mygo.App.WhenReady(func() {
		mygo.App.OnWindowAllClosed(func() {})
		mainVisible := false
		refreshMemory := installMemoryPolicy(func() bool { return mainVisible })
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
			return registerMainShortcut(key, func() {
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
				if err := changeMainHotkeys(pasteKeys[setting], key, func(key string) error { return registerPaste(setting, key) }, unregisterMainShortcut, func() error { return nil }); err != nil {
					log.Printf("Paste shortcut unavailable: %v", err)
				}
			}
		}
		reachable := func() bool {
			return len(registered) > 0 || len(searchKeys) > 0 || (tray != nil && trayHostAvailable(tray))
		}
		var toggle func()
		var present func(string)
		activationContext, cancelActivation := context.WithCancel(context.Background())
		mygo.App.OnWillQuit(func(*mygo.QuitEvent) { cancelActivation() })
		activating := false
		activationCommand := ""
		activationStarted := time.Time{}
		registerSearch := func(key string) error {
			return registerMainShortcut(key, func() {
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
			if err := changeMainHotkeys(searchKeys, key, registerSearch, unregisterMainShortcut, func() error { return nil }); err != nil {
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
		presentReady := func(command string) {
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
			mainChild = child
			done := make(chan struct{})
			mainDone = done
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
					if value, found := strings.CutPrefix(line, "tray-show:"); found {
						if request, err := strconv.ParseUint(value, 10, 64); err == nil && request > 0 {
							mygo.RunOnMain(func() { present(line) })
						}
						continue
					}
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
									log.Print("Settings reset requested by main window")
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
									log.Print("Settings reset acknowledged; rebuilding desktop")
									reply(nil)
									_, _ = io.WriteString(input, "rebuild\n")
								case "appearance":
									if request.Appearance == nil || !request.Appearance.Valid() {
										reply(fmt.Errorf("InvalidNativeAppearance"))
									} else {
										saveNativeAppearance(owner, panel, *request.Appearance)
										reply(nil)
									}
								case "paste-hotkey":
									keys, ok := pasteKeys[request.Panel]
									if !ok {
										reply(fmt.Errorf("InvalidActionHotkey"))
										break
									}
									reply(changeMainHotkeys(keys, request.Hotkey, func(key string) error { return registerPaste(request.Panel, key) }, unregisterMainShortcut, func() error {
										return persistActionHotkey(owner, request.Panel, request.Hotkey, request.Epoch)
									}))
								case "search-hotkey":
									reply(changeMainHotkeys(searchKeys, request.Hotkey, registerSearch, unregisterMainShortcut, func() error {
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
										return registerMainShortcut(key, func() {
											if !recorder.offer(key) {
												toggle()
											}
										})
									}, unregisterMainShortcut, func() error { return persistMainHotkeys(owner, request.Hotkey, request.Epoch) }))
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
					case "tray-hide":
						mygo.RunOnMain(func() {
							if strings.HasPrefix(activationCommand, "tray-show") {
								activationCommand = ""
								activationStarted = time.Time{}
							}
						})
					case "visible", "hidden":
						mygo.RunOnMain(func() {
							mainVisible = line == "visible"
							if mainVisible && !activationStarted.IsZero() {
								desktopTiming("activation to visible", activationStarted)
								activationStarted = time.Time{}
							}
							refreshMemory()
						})
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
				close(done)
				mygo.RunOnMain(func() {
					_ = recorder.stop()
					input.Close()
					commands = nil
					mainPID = 0
					mainVisible = false
					refreshMemory()
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
		present = func(command string) {
			if stopping || migrating || moved || relaunch {
				return
			}
			if pending != nil {
				reconnect(nil)
				return
			}
			activationCommand = command
			if activating {
				return
			}
			activating = true
			activationStarted = time.Now()
			go func() {
				ctx, cancel := context.WithTimeout(activationContext, 30*time.Second)
				defer cancel()
				changed, err := owner.RecoverDesktopForPresentation(ctx)
				mygo.RunOnMain(func() {
					activating = false
					if activationContext.Err() != nil || stopping || migrating || moved || relaunch {
						return
					}
					if err != nil {
						log.Printf("Desktop Host recovery failed: %v", err)
						if strings.HasPrefix(activationCommand, "tray-show") && commands != nil {
							_, _ = io.WriteString(commands, "tray-reset\n")
						}
						if activationCommand == "" {
							return
						}
						_, _ = mygo.Dialog.Message(mygo.MessageOptions{Title: "DezirClip", Message: "无法连接剪贴板服务，请稍后重试。\nClipboard Service is unavailable. Please try again.", Buttons: []string{"OK"}})
						return
					}
					if changed {
						log.Print("Desktop Host changed; replacing native and WebView sessions")
						relaunchCommand = recoveredActivation(activationCommand)
						relaunch = true
						mygo.App.Quit()
						return
					}
					if activationCommand != "" {
						presentReady(activationCommand)
					}
				})
			}()
		}
		toggle = func() { present("toggle") }
		open := func() { present("show") }
		trayToggle := func() {
			if stopping || migrating || moved || relaunch {
				return
			}
			if commands == nil || closing {
				if activating && strings.HasPrefix(activationCommand, "tray-show") {
					activationCommand = ""
					activationStarted = time.Time{}
				} else {
					present("tray-show")
				}
				return
			}
			if _, err := io.WriteString(commands, "tray-toggle\n"); err != nil {
				log.Printf("Tray toggle unavailable: %v", err)
				open()
			}
		}
		startupSettings := refreshMainShortcuts(owner, registered, toggle, recorder.offer)
		restoreNativeAppearance(owner, panel, startupSettings)
		panel.SetAppearanceRefresher(func() {
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			if encoded, err := (&Library{owner}).Preferences(ctx); err == nil {
				var values map[string]string
				if json.Unmarshal(encoded, &values) == nil {
					restoreNativeAppearance(owner, panel, values)
				}
			}
		})
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
			tray.OnClick(trayToggle)
			tray.SetMenu(mygo.NewMenu([]*mygo.MenuItem{
				{Label: "显示／隐藏", Click: func(*mygo.MenuItem, *mygo.Window) { trayToggle() }},
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
		resetContext, stopResetWatch := context.WithCancel(context.Background())
		mygo.App.OnWillQuit(func(*mygo.QuitEvent) { stopResetWatch() })
		go watchSettingsReset(resetContext, owner, func() {
			mygo.RunOnMain(func() {
				if resetContext.Err() != nil || stopping || migrating || moved || pending != nil || relaunch {
					return
				}
				relaunch, closing = true, true
				if commands == nil {
					mygo.App.Quit()
				} else {
					_, _ = io.WriteString(commands, "rebuild\n")
				}
			})
		}, func() {
			mygo.RunOnMain(func() {
				if resetContext.Err() != nil || stopping || migrating || moved || pending != nil || relaunch {
					return
				}
				panel.RefreshShortcut()
				selector.Refresh()
				settings := refreshMainShortcuts(owner, registered, toggle, recorder.offer)
				refreshSearch(settings)
				refreshPaste(settings)
				if settings != nil {
					_ = setTrayVisible(settings["app.hide_tray_icon"] != "true")
				}
			})
		})
		mygo.App.OnBeforeQuit(func(event *mygo.QuitEvent) {
			log.Printf("Desktop quit requested (migrating=%t, relaunch=%t)", migrating, relaunch)
			if migrating {
				event.PreventDefault()
				return
			}
			_ = recorder.stop()
			stopping = true
			unregisterMiddleShortcut()
			for key := range registered {
				unregisterMainShortcut(key)
			}
			if commands != nil {
				commands.Close()
			}
		})
		silent := startupSettings != nil && startupSettings["app.silent_start"] != "false"
		if activation != "" {
			present(activation)
		} else if (!minimized && !silent) || !reachable() {
			open()
		}
		refreshMemory()
	})
	if err := mygo.App.Run(); err != nil {
		return fmt.Errorf("native desktop: %w", err)
	}
	log.Print("Desktop native loop stopped; waiting for main process")
	if mainChild != nil {
		if err := waitDesktopExit(mainDone, mainChild.Process.Kill, 5*time.Second); err != nil {
			return err
		}
	}
	if relaunch {
		child := exec.Command(executable, "--"+relaunchCommand)
		child.Stderr = os.Stderr
		if err := child.Start(); err != nil {
			return err
		}
		if err := grantMainFocus(child.Process.Pid); err != nil {
			log.Printf("Replacement desktop activation permission unavailable: %v", err)
		}
		return child.Process.Release()
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	log.Print("Desktop main process stopped; stopping authenticated Service")
	return owner.StopDesktopService(ctx)
}
