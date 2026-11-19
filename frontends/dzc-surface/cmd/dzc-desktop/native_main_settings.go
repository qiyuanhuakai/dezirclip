package main

import (
	"context"
	"strconv"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type nativeSettingsPane struct {
	uikit.SettingsSection
	draw func()
}

func (n *nativeMain) openSettings() {
	if n.showSettings {
		return
	}
	n.resetPreviewSelection()
	if n.settingsSection == "privacy" {
		n.settingsSection = "general"
		if general := n.settingsOpen["general"]; general != nil {
			*general = true
		}
	}
	if privacy := n.settingsOpen["privacy"]; privacy != nil {
		*privacy = false
	}
	n.showSettings, n.showEmoji = true, false
}

func (n *nativeMain) settingsView(c *ui.Context, kit uikit.Kit, surfaceOpacity *int) {
	panes := []nativeSettingsPane{
		{uikit.SettingsSection{ID: "general", Label: n.text("general_settings", "General")}, func() { n.generalSettings(c, kit) }},
		{uikit.SettingsSection{ID: "clipboard", Label: n.text("clipboard_settings")}, func() { n.clipboardSettings(c, kit) }},
		{uikit.SettingsSection{ID: "privacy", Label: n.text("privacy_settings")}, func() { n.privacySettings(c, kit) }},
		{uikit.SettingsSection{ID: "appearance", Label: n.text("appearance_settings")}, func() { n.appearanceSettings(c, kit, surfaceOpacity) }},
		{uikit.SettingsSection{ID: "applications", Label: n.text("default_apps")}, func() { n.defaultAppsSettings(c) }},
		{uikit.SettingsSection{ID: "tools", Label: n.text("tools_settings")}, func() { n.toolsSettings(c) }},
		{uikit.SettingsSection{ID: "data", Label: n.text("data_management")}, func() { n.dataSettings(c) }},
		{uikit.SettingsSection{ID: "about", Label: n.text("about_settings")}, func() { n.aboutSettings(c, kit) }},
	}
	if n.settingsSection == "" {
		n.settingsSection = "general"
	}
	width, _ := c.Size()
	if width < 560 {
		n.settingsAccordion(c, kit, panes)
		return
	}
	ui.Row(c).Grow(1).Gap(10).Padding(8).Children(func() {
		sections := make([]uikit.SettingsSection, 0, len(panes))
		for _, pane := range panes {
			sections = append(sections, pane.SettingsSection)
		}
		kit.SettingsRail(c, n.text("native_settings_sections"), n.settingsSection, sections, func(id string) {
			n.settingsSection = id
		})
		for _, pane := range panes {
			if pane.ID == n.settingsSection {
				ui.Scroll(c).Grow(1).Children(func() {
					ui.Column(c).FillWidth().Gap(8).Padding(4, 8, 12).Children(func() {
						ui.Text(c, pane.Label).FontWeight(600)
						pane.draw()
					})
				})
				break
			}
		}
	})
	ui.Box(c).FillWidth().Padding(0, 8, 8).Children(func() { n.settingsFooter(c, kit, true) })
}

func (n *nativeMain) settingsAccordion(c *ui.Context, kit uikit.Kit, panes []nativeSettingsPane) {
	if n.settingsOpen == nil {
		n.settingsOpen = make(map[string]*bool, len(panes))
		for _, pane := range panes {
			open := pane.ID == n.settingsSection
			n.settingsOpen[pane.ID] = &open
		}
	}
	ui.Scroll(c).Grow(1).Children(func() {
		ui.Column(c).FillWidth().Gap(8).Padding(8).Children(func() {
			for _, pane := range panes {
				open := n.settingsOpen[pane.ID]
				wasOpen := *open
				kit.SettingsDisclosure(c, pane.Label, open, func() {
					ui.Column(c).FillWidth().Gap(8).Padding(8, 4).Children(pane.draw)
				})
				if *open && !wasOpen {
					n.settingsSection = pane.ID
				}
			}
			n.settingsFooter(c, kit, false)
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
		if ui.Checkbox(c, &enabled, setting.label).Disabled(n.mutating).Changed() {
			n.saveSetting(setting.key, strconv.FormatBool(enabled))
		}
	}
}

func (n *nativeMain) settingEnabled(key string, initial bool) bool {
	if setting, ok := n.preferences[key]; ok {
		return setting == "true"
	}
	return initial
}

func (n *nativeMain) generalSettings(c *ui.Context, kit uikit.Kit) {
	n.settingSelect(c, kit, "app.language", n.text("language"), string(n.language()), []uikit.SelectOption{
		{Value: "zh", Label: "简体中文"},
		{Value: "tw", Label: "繁體中文"},
		{Value: "en", Label: "English"},
	})
	n.booleanSettings(c,
		nativeBooleanSetting{"app.silent_start", n.text("silent_start", "Start hidden"), true},
		nativeBooleanSetting{"app.follow_mouse", n.text("follow_mouse", "Show window near the mouse"), true},
		nativeBooleanSetting{"app.show_search_box", n.text("show_search_box", "Show search box"), true},
		nativeBooleanSetting{"app.show_scroll_top_button", n.text("scroll_top_button", "Show scroll-to-top button"), true},
		nativeBooleanSetting{"app.tag_manager_enabled", n.text("tag_manager_enabled", "Enable tag manager"), true},
		nativeBooleanSetting{"app.emoji_panel_enabled", n.text("emoji_panel_enabled", "Enable emoji panel"), false},
		nativeBooleanSetting{"app.arrow_key_selection", n.text("arrow_key_selection", "Select history with arrow keys"), false},
		nativeBooleanSetting{"app.preview_keyboard_enabled", n.text("preview_keyboard_enabled", "Preview keyboard selection"), false},
	)
	const edge = "app.edge_docking"
	dock := n.settingEnabled(edge, false)
	if ui.Checkbox(c, &dock, n.text("edge_docking", "Edge docking")).Disabled(n.mutating || !dock && !dockingAvailable()).Changed() {
		n.saveSetting(edge, strconv.FormatBool(dock))
	}
	hideTray := n.settingEnabled("app.hide_tray_icon", false)
	if ui.Checkbox(c, &hideTray, n.text("hide_tray_icon", "Hide tray icon")).Disabled(n.mutating).Changed() {
		n.writeSetting(func(ctx context.Context, epoch uint64) error {
			return n.shell.SetTrayVisible(ctx, !hideTray, epoch)
		})
	}
	n.idleSettings(c)
	n.soundSettings(c)
}

func (n *nativeMain) idleSettings(c *ui.Context) {
	const enabled = "app.idle_destroy_enabled"
	n.booleanSettings(c, nativeBooleanSetting{enabled, n.text("idle_destroy_enabled"), false})
	if n.settingEnabled(enabled, false) {
		n.numberSetting(c, "app.idle_destroy_seconds", n.text("idle_destroy_seconds"), defaultIdleSeconds, minIdleSeconds, maxIdleSeconds)
	}
}

func (n *nativeMain) soundSettings(c *ui.Context) {
	const enabled = "app.sound_enabled"
	n.booleanSettings(c, nativeBooleanSetting{enabled, n.text("sound_effects"), false})
	if n.settingEnabled(enabled, false) {
		volume := nativeRangeSetting{key: "app.sound_volume", unit: "%", maximum: 100, fallback: 70}
		n.rangeSetting(c, volume, n.text("sound_volume"), volume.value(n.preferences))
		n.booleanSettings(c, nativeBooleanSetting{"app.sound_paste_enabled", n.text("paste_sound"), true})
	}
}

func (n *nativeMain) clipboardSettings(c *ui.Context, kit uikit.Kit) {
	n.retentionSettings(c)
	n.clipboardShortcuts(c)
	n.booleanSettings(c,
		nativeBooleanSetting{"app.deduplicate", n.text("merge_duplicates", "Merge duplicate entries"), true},
		nativeBooleanSetting{"app.capture_files", n.text("capture_files", "Capture copied files"), false},
		nativeBooleanSetting{"app.capture_rich_text", n.text("capture_rich_text"), false},
		nativeBooleanSetting{"app.delete_after_paste", n.text("delete_after_paste", "Delete entries after pasting"), false},
		nativeBooleanSetting{"app.move_to_top_after_paste", n.text("move_to_top_after_paste", "Move pasted entries to top"), true},
	)
	n.pasteMethodSetting(c, kit)
	n.sequentialSettings(c)
}

func (n *nativeMain) privacySettings(c *ui.Context, kit uikit.Kit) {
	private := n.private
	if ui.Checkbox(c, &private, n.text("privacy_session_title", "Private session")).Disabled(n.mutating).Changed() {
		n.changePrivateSession(private)
	}
	n.shortcutSetting(c, "app.privacy_session_hotkey", n.text("privacy_session_hotkey"), "")
	n.booleanSettings(c, nativeBooleanSetting{"app.privacy_protection", n.text("privacy_protection", "Protect sensitive clipboard content"), true})
	n.drawPrivacyRules(c)
	n.drawPrivacyDetection(c, kit)
}

func (n *nativeMain) changePrivateSession(active bool) {
	if n.session == nil || n.mutating || n.history.state.ServiceInstanceId == "" {
		return
	}
	n.resetPreviewSelection()
	n.history.clear()
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
				n.loadPage()
				return
			}
			n.prepare(n.resume)
		})
	}()
}

func (n *nativeMain) appearanceSettings(c *ui.Context, kit uikit.Kit, surfaceOpacity *int) {
	themes := []uikit.SelectOption{
		{Value: "retro", Label: n.text("theme_retro")}, {Value: "mica", Label: n.text("theme_mica")}, {Value: "acrylic", Label: n.text("theme_acrylic")},
		{Value: "liquid-glass", Label: n.text("theme_liquid_glass")}, {Value: "macos", Label: n.text("theme_macos")}, {Value: "scifi", Label: n.text("theme_scifi")},
	}
	for _, imported := range n.importedThemes {
		themes = append(themes, uikit.SelectOption{Value: "custom:" + imported.Id, Label: imported.Name})
	}
	n.settingSelect(c, kit, "app.theme", n.text("visual_theme", "Theme"), "mica", themes)
	n.themeActions(c)
	n.settingSelect(c, kit, "app.color_mode", n.text("color_mode", "Color mode"), "system", []uikit.SelectOption{
		{Value: "system", Label: n.text("mode_system")}, {Value: "light", Label: n.text("mode_light")}, {Value: "dark", Label: n.text("mode_dark")},
	})
	n.booleanSettings(c,
		nativeBooleanSetting{"app.compact_mode", n.text("compact_mode", "Compact history"), false},
		nativeBooleanSetting{"app.show_app_border", n.text("show_app_border", "Show window border"), true},
	)
	n.typographySettings(c)
	previewOpacity := opacityPreference(n.preferences["app.preview_opacity"], n.preferences["app.surface_opacity"])
	n.rangeSetting(c, nativePreviewOpacity, n.text("preview_opacity"), previewOpacity)
	if surfaceOpacity != nil {
		*surfaceOpacity = n.rangeSetting(c, nativeSurfaceOpacity, n.text("surface_opacity", "Surface opacity"), *surfaceOpacity)
	}
}

func (n *nativeMain) settingSelect(c *ui.Context, kit uikit.Kit, key, label, fallback string, options []uikit.SelectOption) {
	ui.Text(c, label).FontWeight(600)
	selected := n.preferences[key]
	if selected == "" {
		selected = fallback
	}
	if kit.Select(c, &selected, label, options, n.mutating).Changed() {
		n.saveSetting(key, selected)
	}
}

func (n *nativeMain) toolsSettings(c *ui.Context) {
	enabled := n.settingEnabled("app.screenshot_enabled", true)
	n.panelCheckbox(c, "screenshot", n.text("screenshot_enabled", "Enable screenshots"), enabled)
	if enabled {
		ui.Row(c).Gap(6).Wrap().Children(func() {
			for _, option := range []struct {
				label string
				full  bool
			}{{n.text("capture_region_now"), false}, {n.text("capture_full_now"), true}} {
				if uikit.Button(c, uikit.ButtonOptions{Label: option.label, Disabled: n.capturing || n.mutating}).Clicked() {
					n.captureScreenshot(option.full)
				}
			}
		})
	}
	if enabled {
		n.shortcutSetting(c, "app.screenshot_hotkey", n.text("screenshot_hotkey"), "Ctrl+Shift+A")
		n.shortcutSetting(c, "app.screenshot_full_hotkey", n.text("screenshot_full_hotkey"), "Ctrl+Shift+F")
	}
	quick := n.settingEnabled("app.quick_paste_enabled", true)
	n.panelCheckbox(c, "quick-paste", n.text("quick_paste_enabled", "Enable quick paste"), quick)
	if quick {
		n.shortcutSetting(c, "app.quick_paste_hotkey", n.text("quick_paste_hotkey_label"), "Ctrl+Shift+V")
	}
	n.booleanSettings(c, nativeBooleanSetting{"app.ocr_enabled", n.text("ocr_enabled", "Recognize text in images"), true})
	n.drawOCREngine(c)
}

func (n *nativeMain) panelCheckbox(c *ui.Context, id, label string, enabled bool) {
	if ui.Checkbox(c, &enabled, label).Disabled(n.mutating || n.capturing).Changed() {
		n.writeSetting(func(ctx context.Context, epoch uint64) error {
			return n.shell.SetNativePanelEnabled(ctx, id, enabled, epoch)
		})
	}
}

func (n *nativeMain) captureScreenshot(full bool) {
	if n.session == nil || n.capturing || n.mutating || n.history.state.ServiceInstanceId == "" {
		return
	}
	n.capturing, n.notice = true, ""
	session, epoch := n.session, n.history.state.PrivacyEpoch
	go func() {
		ctx, cancel := context.WithTimeout(session, 115*time.Second)
		defer cancel()
		err := n.shell.Capture(ctx, full, epoch)
		n.update(session, func() {
			n.capturing = false
			if err != nil {
				n.notice = n.text("screenshot_failed", "Could not capture a screenshot.")
			}
		})
	}()
}
