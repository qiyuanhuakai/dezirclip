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
	}
	if privacy := n.settingsOpen["privacy"]; privacy != nil {
		*privacy = false
	}
	n.showSettings, n.showEmoji = true, false
}

func (n *nativeMain) settingsView(c *ui.Context, kit uikit.Kit) {
	panes := []nativeSettingsPane{
		{uikit.SettingsSection{ID: "general", Label: "General"}, func() { n.generalSettings(c) }},
		{uikit.SettingsSection{ID: "clipboard", Label: "Clipboard"}, func() { n.clipboardSettings(c) }},
		{uikit.SettingsSection{ID: "privacy", Label: "Privacy"}, func() { n.privacySettings(c) }},
		{uikit.SettingsSection{ID: "appearance", Label: "Appearance"}, func() { n.appearanceSettings(c, kit) }},
		{uikit.SettingsSection{ID: "tools", Label: "Tools"}, func() { n.toolsSettings(c) }},
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
		kit.SettingsRail(c, "Settings sections", n.settingsSection, sections, func(id string) {
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
		})
	})
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

func (n *nativeMain) generalSettings(c *ui.Context) {
	n.booleanSettings(c,
		nativeBooleanSetting{"app.silent_start", "Start hidden", true},
		nativeBooleanSetting{"app.follow_mouse", "Show window near the mouse", true},
		nativeBooleanSetting{"app.edge_docking", "Edge docking", false},
		nativeBooleanSetting{"app.show_search_box", "Show search box", true},
		nativeBooleanSetting{"app.show_scroll_top_button", "Show scroll-to-top button", true},
		nativeBooleanSetting{"app.tag_manager_enabled", "Enable tag manager", true},
		nativeBooleanSetting{"app.emoji_panel_enabled", "Enable emoji panel", false},
		nativeBooleanSetting{"app.arrow_key_selection", "Select history with arrow keys", false},
		nativeBooleanSetting{"app.preview_keyboard_enabled", "Preview keyboard selection", false},
	)
	hideTray := n.settingEnabled("app.hide_tray_icon", false)
	if ui.Checkbox(c, &hideTray, "Hide tray icon").Disabled(n.mutating).Changed() {
		n.writeSetting(func(ctx context.Context, epoch uint64) error {
			return n.shell.SetTrayVisible(ctx, !hideTray, epoch)
		})
	}
}

func (n *nativeMain) clipboardSettings(c *ui.Context) {
	n.booleanSettings(c,
		nativeBooleanSetting{"app.persistent", "Keep clipboard history", false},
		nativeBooleanSetting{"app.persistent_limit_enabled", "Limit stored clipboard entries", true},
		nativeBooleanSetting{"app.deduplicate", "Merge duplicate entries", true},
		nativeBooleanSetting{"app.capture_files", "Capture copied files", false},
		nativeBooleanSetting{"app.capture_rich_text", "Capture formatted text", false},
		nativeBooleanSetting{"app.delete_after_paste", "Delete entries after pasting", false},
		nativeBooleanSetting{"app.move_to_top_after_paste", "Move pasted entries to top", true},
	)
}

func (n *nativeMain) privacySettings(c *ui.Context) {
	private := n.private
	if ui.Checkbox(c, &private, "Private session").Disabled(n.mutating).Changed() {
		n.changePrivateSession(private)
	}
	n.booleanSettings(c, nativeBooleanSetting{"app.privacy_protection", "Protect sensitive clipboard content", true})
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
				n.notice = "Could not change the private session."
				n.loadPage()
				return
			}
			n.prepare(n.resume)
		})
	}()
}

func (n *nativeMain) appearanceSettings(c *ui.Context, kit uikit.Kit) {
	n.settingSelect(c, kit, "app.theme", "Theme", "mica", []uikit.SelectOption{
		{Value: "retro", Label: "Retro"}, {Value: "mica", Label: "Mica"}, {Value: "acrylic", Label: "Acrylic"},
		{Value: "liquid-glass", Label: "Liquid Glass"}, {Value: "macos", Label: "macOS"}, {Value: "scifi", Label: "Sci-Fi"},
	})
	n.settingSelect(c, kit, "app.color_mode", "Color mode", "system", []uikit.SelectOption{
		{Value: "system", Label: "System"}, {Value: "light", Label: "Light"}, {Value: "dark", Label: "Dark"},
	})
	n.booleanSettings(c,
		nativeBooleanSetting{"app.compact_mode", "Compact history", false},
		nativeBooleanSetting{"app.show_app_border", "Show window border", true},
	)
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
	n.panelCheckbox(c, "screenshot", "Enable screenshots", enabled)
	if enabled {
		ui.Row(c).Gap(6).Wrap().Children(func() {
			for _, option := range []struct {
				label string
				full  bool
			}{{"Region Capture", false}, {"Full Screen Capture", true}} {
				if uikit.Button(c, uikit.ButtonOptions{Label: option.label, Disabled: n.capturing || n.mutating}).Clicked() {
					n.captureScreenshot(option.full)
				}
			}
		})
	}
	n.panelCheckbox(c, "quick-paste", "Enable quick paste", n.settingEnabled("app.quick_paste_enabled", true))
	n.booleanSettings(c, nativeBooleanSetting{"app.ocr_enabled", "Recognize text in images", true})
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
				n.notice = "Could not capture a screenshot."
			}
		})
	}()
}
