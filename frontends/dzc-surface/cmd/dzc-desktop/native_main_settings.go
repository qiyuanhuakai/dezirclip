package main

import (
	"context"
	"runtime"
	"strconv"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/uikit"
)

type nativeSettingsPane struct {
	uikit.SettingsSection
	draw func()
}

const nativeSplitWidth = 560

func (n *nativeMain) openSettings() {
	if n.showSettings {
		return
	}
	n.resetPreviewSelection()
	n.settingsScroll = ui.ScrollState{}
	n.settingsStackScroll = ui.ScrollState{}
	if n.settingsSection == "privacy" {
		n.settingsSection = "general"
	}
	for _, expanded := range n.settingsOpen {
		*expanded = false
	}
	n.showSettings, n.showEmoji = true, false
}

func (n *nativeMain) closeSettings() {
	if !n.profile.moving && !n.profile.restartPending {
		n.showSettings = false
		n.focusPrivacyRules = false
		n.updater.confirm = false
		n.resetOpacityPreviews()
	}
}

func (n *nativeMain) settingsView(c *ui.Context, kit uikit.Kit, surfaceOpacity *int) {
	const horizontalInset, splitGap = 8, 12
	width, _ := c.Size()
	cardWidth := min(width-2*horizontalInset, uikit.SettingsStackMaxWidth)
	if width >= nativeSplitWidth {
		cardWidth = width - 2*horizontalInset - splitGap - kit.SettingsRailWidth()
	}
	if n.preferences["app.show_app_border"] != "false" {
		cardWidth -= 2
	}
	panes := []nativeSettingsPane{
		{uikit.SettingsSection{ID: "general", Label: n.text("general_settings", "General")}, func() { n.generalSettings(c) }},
		{uikit.SettingsSection{ID: "clipboard", Label: n.text("clipboard_settings")}, func() { n.clipboardSettings(c, kit) }},
		{uikit.SettingsSection{ID: "privacy", Label: n.text("privacy_settings")}, func() { n.privacySettings(c, kit) }},
		{uikit.SettingsSection{ID: "appearance", Label: n.text("appearance_settings")}, func() {
			n.appearanceSettings(c, kit, kit.SettingsSectionContentWidth(cardWidth), surfaceOpacity)
		}},
		{uikit.SettingsSection{ID: "applications", Label: n.text("default_apps")}, func() { n.defaultAppsSettings(c) }},
		{uikit.SettingsSection{ID: "data", Label: n.text("data_management")}, func() { n.dataSettings(c) }},
		{uikit.SettingsSection{ID: "tools", Label: n.text("tools_settings")}, func() { n.toolsSettings(c, kit) }},
		{uikit.SettingsSection{ID: "about", Label: n.text("about_settings")}, func() { n.aboutSettings(c, kit) }},
	}
	if n.settingsSection == "" {
		n.settingsSection = "general"
	}
	if width < nativeSplitWidth {
		n.settingsAccordion(c, kit, panes)
		return
	}
	ui.Row(c).FillWidth().Grow(1).Shrink(1).MinHeight(0).AlignItems(ui.Stretch).Gap(splitGap).Padding(12, horizontalInset).Children(func() {
		sections := make([]uikit.SettingsSection, 0, len(panes))
		for _, pane := range panes {
			sections = append(sections, pane.SettingsSection)
		}
		ui.Scroll(c).Shrink(0).FillHeight().MinHeight(0).Children(func() {
			kit.SettingsRail(c, n.text("native_settings_sections"), n.settingsSection, sections, func(id string) {
				n.settingsSection = id
				if id != "appearance" {
					n.resetOpacityPreviews()
				}
				n.settingsScroll = ui.ScrollState{}
			})
		})
		for _, pane := range panes {
			if pane.ID == n.settingsSection {
				ui.Scroll(c.Key(pane.ID)).Grow(1).MinWidth(0).FillHeight().MinHeight(0).
					TrackScroll(&n.settingsScroll).Transition(nativePanelEnter).Children(func() {
					if pane.ID == "data" || pane.ID == "about" {
						kit.SettingsSectionCard(c, pane.Label, nil, pane.draw)
						return
					}
					kit.SettingsSectionGrid(c, pane.Label, width, pane.draw)
				})
				break
			}
		}
	})
	ui.Box(c).FillWidth().Padding(0, horizontalInset, 8).Children(func() { n.settingsFooter(c, kit, true) })
}

func (n *nativeMain) settingsAccordion(c *ui.Context, kit uikit.Kit, panes []nativeSettingsPane) {
	if n.settingsOpen == nil {
		n.settingsOpen = make(map[string]*bool, len(panes))
		for _, pane := range panes {
			open := pane.ID == "privacy" && pane.ID == n.settingsSection
			n.settingsOpen[pane.ID] = &open
		}
	}
	ui.Scroll(c).FillWidth().Grow(1).Shrink(1).MinHeight(0).AlignItems(ui.Center).TrackScroll(&n.settingsStackScroll).Children(func() {
		ui.Column(c).FillWidth().Padding(12, 8).Children(func() {
			uikit.SettingsSectionStack(c, func() {
				for _, pane := range panes {
					open := n.settingsOpen[pane.ID]
					wasOpen := *open
					kit.SettingsSectionCard(c, pane.Label, open, pane.draw)
					if *open && !wasOpen {
						n.settingsSection = pane.ID
					} else if !*open && wasOpen && pane.ID == "appearance" {
						n.resetOpacityPreviews()
					}
				}
				n.settingsFooter(c, kit, false)
			})
		})
	})
}

func (n *nativeMain) settingsFooter(c *ui.Context, kit uikit.Kit, compact bool) {
	open := func(address string) {
		if err := n.shell.OpenURL(address); err != nil {
			n.notice = n.text("common_error")
		}
	}
	kit.SettingsFooter(c, uikit.SettingsFooterOptions{
		ProductName: "DezirClip", Feedback: n.text("feedback"), Website: n.text("official_website"),
		Reset: n.text("reset_defaults"), Compact: compact,
		ResetDisabled: n.session == nil || n.history.state.ServiceInstanceId == "" || n.mutating || n.capturing,
		OnFeedback:    func() { open(desktopProjectURL + "/issues") },
		OnReset:       func() { n.resetOpen = true },
		OnWebsite:     func() { open(desktopProjectURL) },
	})
}

func (n *nativeMain) confirmSettingsReset() {
	state := n.history.state
	if n.session == nil || state.ServiceInstanceId == "" || n.mutating || n.capturing {
		return
	}
	n.mutating, n.notice = true, ""
	session := n.session
	go func() {
		err := n.shell.ResetSettings(session, state.PrivacyEpoch)
		n.update(session, func() {
			if err != nil {
				n.mutating = false
				n.notice = n.text("reset_failed") + err.Error()
			}
		})
	}()
}

type nativeBooleanSetting struct {
	key, label string
	initial    bool
}

func (n *nativeMain) booleanSettings(c *ui.Context, settings ...nativeBooleanSetting) {
	for _, setting := range settings {
		enabled := n.settingEnabled(setting.key, setting.initial)
		if n.mainKit().SettingsToggle(c, &enabled, setting.label).Disabled(n.mutating).Changed() {
			n.saveSetting(setting.key, strconv.FormatBool(enabled))
		}
	}
}

func (n *nativeMain) booleanSettingWithHelp(c *ui.Context, key, label, hint string, initial bool) {
	enabled := n.settingEnabled(key, initial)
	if n.mainKit().SettingsToggleField(c.Key(key), &enabled, label, n.settingsHelpLabel(label), hint, n.mutating) {
		n.saveSetting(key, strconv.FormatBool(enabled))
	}
}

func (n *nativeMain) settingEnabled(key string, initial bool) bool {
	if setting, ok := n.preferences[key]; ok {
		return setting == "true"
	}
	return initial
}

func (n *nativeMain) generalSettings(c *ui.Context) {
	kit := n.mainKit()
	n.loginSettings(c.Key("login-startup"))
	const trayKey = "app.hide_tray_icon"
	hideTray := n.settingEnabled(trayKey, false)
	if kit.SettingsToggleField(c.Key(trayKey), &hideTray, n.text("hide_tray_icon"), "", "", n.mutating) {
		n.writeSettingWithEffect(func(ctx context.Context, epoch uint64) error {
			return n.shell.SetTrayVisible(ctx, !hideTray, epoch)
		}, func() { n.applySavedSetting(trayKey, strconv.FormatBool(hideTray)) })
	}
	const edge = "app.edge_docking"
	dock, dockLabel := n.settingEnabled(edge, false), n.text("edge_docking")
	if kit.SettingsToggleField(c.Key(edge), &dock, dockLabel, n.settingsHelpLabel(dockLabel), n.text("edge_docking_hint"),
		n.mutating || !dock && !dockingAvailable()) {
		n.saveSetting(edge, strconv.FormatBool(dock))
	}
	n.booleanSettingWithHelp(c, "app.follow_mouse", n.text("follow_mouse"), "", true)
	n.idleSettings(c)
	n.soundSettings(c)
	n.booleanSettingWithHelp(c, "app.silent_start", n.text("silent_start"), n.text("silent_start_hint"), true)
	n.booleanSettingWithHelp(c, "app.show_search_box", n.text("show_search_box"), n.text("show_search_box_hint"), true)
	n.booleanSettingWithHelp(c, "app.show_scroll_top_button", n.text("scroll_top_button"), n.text("scroll_top_button_hint"), true)
	n.booleanSettingWithHelp(c, "app.emoji_panel_enabled", n.text("emoji_panel_enabled"), n.text("emoji_panel_enabled_hint"), false)
	n.booleanSettingWithHelp(c, "app.tag_manager_enabled", n.text("tag_manager_enabled"), n.text("tag_manager_enabled_hint"), true)
	const arrowKeySelection = "app.arrow_key_selection"
	n.booleanSettingWithHelp(c, arrowKeySelection, n.text("arrow_key_selection"), n.text("arrow_key_selection_hint"), false)
	const previewKey = "app.preview_keyboard_enabled"
	preview := n.settingEnabled(previewKey, false)
	label := n.text("preview_keyboard_enabled")
	if kit.SettingsDetailToggle(c.Key(previewKey), &preview, label, n.settingsHelpLabel(label), n.text("preview_keyboard_enabled_hint"),
		n.mutating || !n.settingEnabled(arrowKeySelection, false)) {
		n.saveSetting(previewKey, strconv.FormatBool(preview))
	}
	if runtime.GOOS == "windows" {
		n.adminRestartSetting(c, kit)
	}
}

func (n *nativeMain) adminRestartSetting(c *ui.Context, kit uikit.Kit) {
	label := n.text("restart_as_admin")
	kit.SettingsField(c.Key("restart-as-admin"), uikit.SettingsFieldOptions{}).Children(func() {
		kit.SettingsInlineLabel(c, label, n.settingsHelpLabel(label), n.text("restart_as_admin_hint"))
		if kit.SettingsDataAction(c, n.text("restart"), "", n.session == nil || n.history.state.ServiceInstanceId == "" || n.mutating).Clicked() {
			n.restartingAdmin, n.mutating, n.adminRestartFailed = true, true, false
			window := n.window
			go func() {
				err := n.shell.RestartAsAdmin(context.Background())
				window.Update(func() {
					n.restartingAdmin, n.mutating, n.adminRestartFailed = false, false, err != nil
				})
			}()
		}
	})
	if n.adminRestartFailed {
		ui.Text(c, n.text("native_restart_as_admin_failed")).TextColor(c.Theme().Danger).Wrap()
	}
}

func (n *nativeMain) idleSettings(c *ui.Context) {
	const enabled = "app.idle_destroy_enabled"
	n.booleanSettingWithHelp(c, enabled, n.text("idle_destroy_enabled"), n.text("idle_destroy_enabled_hint"), false)
	if n.settingEnabled(enabled, false) {
		n.numberSetting(c, "app.idle_destroy_seconds", n.text("idle_destroy_seconds"), n.text("idle_destroy_seconds_hint"),
			defaultIdleSeconds, minIdleSeconds, maxIdleSeconds)
	}
}

func (n *nativeMain) soundSettings(c *ui.Context) {
	const enabled = "app.sound_enabled"
	n.booleanSettingWithHelp(c, enabled, n.text("sound_effects"), "", false)
	if n.settingEnabled(enabled, false) {
		volume := nativeRangeSetting{key: "app.sound_volume", unit: "%", maximum: 100, fallback: 70}
		n.rangeSetting(c, volume, n.text("sound_volume"), "", volume.value(n.preferences))
		n.booleanSettingWithHelp(c, "app.sound_paste_enabled", n.text("paste_sound"), "", true)
	}
}

func (n *nativeMain) clipboardSettings(c *ui.Context, kit uikit.Kit) {
	n.retentionSettings(c)
	n.booleanSettingWithHelp(c, "app.deduplicate", n.text("merge_duplicates", "Merge duplicate entries"), n.text("merge_duplicates_hint"), true)
	n.booleanSettings(c, nativeBooleanSetting{"app.capture_files", n.text("capture_files", "Capture copied files"), false})
	n.booleanSettingWithHelp(c, "app.capture_rich_text", n.text("capture_rich_text"), n.text("capture_rich_text_hint"), false)
	n.shortcutSetting(c, "app.rich_paste_hotkey", n.text("rich_paste_hotkey_label"), "Ctrl+Shift+Z")
	n.shortcutSetting(c, "app.search_hotkey", n.text("search_hotkey_label"), "Alt+F")
	n.booleanSettingWithHelp(c, "app.delete_after_paste", n.text("delete_after_paste", "Delete entries after pasting"), "", false)
	n.booleanSettingWithHelp(c, "app.move_to_top_after_paste", n.text("move_to_top_after_paste", "Move pasted entries to top"), n.text("move_to_top_after_paste_hint"), true)
	n.pasteMethodSetting(c, kit)
	n.sequentialSettings(c)
	n.globalShortcutSettings(c)
}

func (n *nativeMain) privacySettings(c *ui.Context, kit uikit.Kit) {
	n.drawPrivacySession(c, kit)
	n.drawPrivacyRules(c)
	n.booleanSettingWithHelp(c, "app.privacy_protection", n.text("privacy_protection"), n.text("privacy_protection_hint"), true)
	n.drawPrivacyDetection(c, kit)
}

func (n *nativeMain) changePrivateSession(active bool) {
	if n.session == nil || n.mutating || n.history.state.ServiceInstanceId == "" {
		return
	}
	n.resetPreviewSelection()
	n.history.clear()
	n.clearBackground()
	n.cancelSearchTimer()
	if n.stopRead != nil {
		n.stopRead()
		n.stopRead = nil
	}
	n.mutating, n.reading, n.notice = true, true, ""
	session := n.session
	go func() {
		ctx, cancel := context.WithTimeout(session, 4*time.Second)
		defer cancel()
		_, err := n.library.SetPrivacy(ctx, protocol.PrivacySetParams{Active: active})
		n.update(session, func() {
			if err != nil {
				n.mutating = false
				n.notice = n.text("privacy_action_failed", "Could not change the private session.")
				n.loadBackground()
				n.loadPage()
				return
			}
			n.prepare(n.resume)
		})
	}()
}

func (n *nativeMain) appearanceSettings(c *ui.Context, kit uikit.Kit, width float32, surfaceOpacity *int) {
	kit.SettingsField(c.Key("theme"), uikit.SettingsFieldOptions{Stacked: true}).ColumnSpan(-1).Children(func() {
		n.themeSettings(c, kit, width)
	})
	n.settingChoices(c, kit, "app.color_mode", n.text("color_mode", "Color mode"), "system", []uikit.SelectOption{
		{Value: "system", Label: n.text("mode_system")}, {Value: "light", Label: n.text("mode_light")}, {Value: "dark", Label: n.text("mode_dark")},
	})
	n.settingChoices(c, kit, "app.language", n.text("language"), string(n.language()), []uikit.SelectOption{
		{Value: "zh", Label: "简体"}, {Value: "tw", Label: "繁體"}, {Value: "en", Label: "English"},
	})
	n.booleanSettingWithHelp(c, "app.show_app_border", n.text("show_app_border", "Show window border"), n.text("show_app_border_hint"), true)
	n.booleanSettingWithHelp(c, "app.compact_mode", n.text("compact_mode", "Compact history"), n.text("compact_mode_hint"), false)
	n.typographySettings(c)
	previewOpacity := opacityPreference(n.preferences["app.preview_opacity"], n.preferences["app.surface_opacity"])
	n.rangeSetting(c, nativePreviewOpacity, n.text("preview_opacity"), n.text("preview_opacity_hint"), previewOpacity)
	n.backgroundSettings(c, kit)
	if surfaceOpacity != nil {
		*surfaceOpacity = n.drawSurfaceOpacity(c, *surfaceOpacity)
	}
}

func (n *nativeMain) drawSurfaceOpacity(c *ui.Context, displayed int) int {
	saved := opacityPreference(n.preferences["app.surface_opacity"])
	live := n.rangeSetting(c, nativeSurfaceOpacity, n.text("surface_opacity", "Surface opacity"), "", saved)
	if live != displayed {
		c.Invalidate()
	}
	if live == saved {
		n.surfaceOpacityDraft = nil
	} else {
		n.surfaceOpacityDraft = &live
	}
	return live
}

func (n *nativeMain) resetOpacityPreviews() {
	n.surfaceOpacityDraft, n.background.opacityDraft = nil, nil
}

func (n *nativeMain) settingChoices(c *ui.Context, kit uikit.Kit, key, label, fallback string, options []uikit.SelectOption) {
	kit.SettingsField(c.Key(key), uikit.SettingsFieldOptions{Stacked: true, Last: key == "app.language"}).Children(func() {
		kit.SettingsLabel(c, label).Margin(0, 0, 8, 0)
		value := n.preferences[key]
		if value == "" {
			value = fallback
		}
		disabled := n.session == nil || n.history.state.ServiceInstanceId == "" || n.mutating
		kit.SettingsChoices(c, label, value, options, false, disabled, func(value string) { n.saveSetting(key, value) })
	})
}

func (n *nativeMain) settingsHelpLabel(label string) string {
	return n.text("settings_help") + ": " + label
}

func (n *nativeMain) toolsSettings(c *ui.Context, kit uikit.Kit) {
	kit.CaptureSettingsHeading(c, n.text("capture_settings"))
	enabled := n.settingEnabled("app.screenshot_enabled", true)
	n.panelCheckbox(c.Key("screenshot-enabled"), kit, "screenshot", n.text("screenshot_enabled"), n.text("screenshot_enabled_hint"), enabled)
	if enabled {
		n.shortcutSetting(c, "app.screenshot_hotkey", n.text("screenshot_hotkey"), "Ctrl+Shift+A")
		n.shortcutSetting(c, "app.screenshot_full_hotkey", n.text("screenshot_full_hotkey"), "Ctrl+Shift+F")
		kit.CaptureSettingsActions(c, uikit.CaptureSettingsActionsOptions{
			Label: n.text("capture_now"), Region: n.text("capture_region_now"), Full: n.text("capture_full_now"),
			Disabled: n.session == nil || n.history.state.ServiceInstanceId == "" || n.capturing || n.mutating,
			OnRegion: func() { n.captureScreenshot(false) }, OnFull: func() { n.captureScreenshot(true) },
			Error: n.captureError,
		})
	}
	quick := n.settingEnabled("app.quick_paste_enabled", true)
	n.panelCheckbox(c.Key("quick-paste-enabled"), kit, "quick-paste", n.text("quick_paste_enabled"), n.text("quick_paste_enabled_hint"), quick)
	if quick {
		n.shortcutSetting(c, "app.quick_paste_hotkey", n.text("quick_paste_hotkey_label"), "Ctrl+Shift+V")
		n.settingChoices(c, kit, "app.quick_paste_layout", n.text("quick_paste_layout"), "list", []uikit.SelectOption{
			{Value: "list", Label: n.text("quick_paste_layout_list")},
			{Value: "ring", Label: n.text("quick_paste_layout_ring")},
		})
		if n.preferences["app.quick_paste_layout"] == "ring" {
			options := make([]uikit.SelectOption, 0, 10)
			for count := 3; count <= 12; count++ {
				value := strconv.Itoa(count)
				options = append(options, uikit.SelectOption{Value: value, Label: value})
			}
			n.settingChoices(c, kit, "app.quick_paste_full_sectors", n.text("quick_paste_full_sectors"), "8", options)
			n.settingChoices(c, kit, "app.quick_paste_half_sectors", n.text("quick_paste_half_sectors"), "5", options[:6])
		}
		kit.FixedCaptureRenderer(c, n.text("quick_paste_renderer"), "MyGo Native")
	}
	n.booleanSettingWithHelp(c, "app.ocr_enabled", n.text("ocr_enabled"), n.text("ocr_enabled_hint"), true)
	n.drawOCREngine(c, kit)
	n.cliSettings(c, kit)
}

func (n *nativeMain) panelCheckbox(c *ui.Context, kit uikit.Kit, id, label, hint string, enabled bool) {
	disabled := n.session == nil || n.history.state.ServiceInstanceId == "" || n.mutating || n.capturing
	if kit.SettingsToggleField(c, &enabled, label, n.settingsHelpLabel(label), hint, disabled) {
		key := "app.screenshot_enabled"
		if id == "quick-paste" {
			key = "app.quick_paste_enabled"
		}
		n.writeSettingWithEffect(func(ctx context.Context, epoch uint64) error {
			return n.shell.SetNativePanelEnabled(ctx, id, enabled, epoch)
		}, func() { n.applySavedSetting(key, strconv.FormatBool(enabled)) })
	}
}

func (n *nativeMain) captureScreenshot(full bool) {
	if n.session == nil || n.capturing || n.mutating || n.history.state.ServiceInstanceId == "" {
		return
	}
	n.capturing, n.captureError, n.notice = true, "", ""
	session, epoch := n.session, n.history.state.PrivacyEpoch
	go func() {
		ctx, cancel := context.WithTimeout(session, 115*time.Second)
		defer cancel()
		err := n.shell.Capture(ctx, full, epoch)
		n.update(session, func() {
			n.capturing = false
			if err != nil {
				n.captureError = n.text("screenshot_failed", "Could not capture a screenshot.")
				if !n.showSettings {
					n.notice = n.captureError
				}
			}
		})
	}()
}
