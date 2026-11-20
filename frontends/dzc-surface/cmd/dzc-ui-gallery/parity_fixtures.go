package main

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

func (g *gallery) parityFixture(c *ui.Context, kit uikit.Kit, name string) bool {
	switch name {
	case "settings-section-card":
		if g.state == "wide" {
			kit.SettingsSectionGrid(c, "General settings", 700, func() {
				kit.SettingsLabel(c, "Start hidden")
				kit.SettingsLabel(c, "Open near pointer")
				path := "C:/Example/Clipboard"
				ui.Column(c).ColumnSpan(-1).Children(func() { kit.TextField(c, &path, "Data path") })
			})
			break
		}
		kit.SettingsSectionCard(c, "General settings", &g.groupOpen, func() {
			kit.SettingsLabel(c, "Keep clipboard history")
		})
	case "settings-choices":
		ui.Column(c).Width(330).Gap(8).Children(func() {
			kit.SettingsLabel(c, "Color mode")
			kit.SettingsChoices(c, "Color mode", g.colorChoice, []uikit.SelectOption{
				{Value: "system", Label: "System"}, {Value: "light", Label: "Light"}, {Value: "dark", Label: "Dark"},
			}, false, false, func(value string) { g.colorChoice = value })
			kit.SettingsLabel(c, "Update channel")
			kit.SettingsChoices(c, "Update channel", g.channelChoice, []uikit.SelectOption{
				{Value: "stable", Label: "Stable"}, {Value: "preview", Label: "Preview"},
			}, true, false, func(value string) { g.channelChoice = value })
		})
	case "hotkey-list":
		kit.HotkeyListEditor(c, "Global hotkey", func() {
			kit.HotkeyBinding(c, "+ Global hotkey", "", "+ Global hotkey", false, false)
		}, func() {
			kit.HotkeyList(c, g.bindings, "Not set", "Delete", false, func(value string) {
				g.bindings = slices.DeleteFunc(g.bindings, func(binding string) bool { return value == binding })
			})
		})
	case "emoji-favorites":
		ui.Row(c).FillWidth().Gap(16).Padding(8, 0).Justify(ui.Center).Children(func() {
			kit.EmojiAddTile(c, "Add emoji", 66, false)
			kit.EmojiFavoriteTile(c, uikit.EmojiFavoriteOptions{
				Name: "Example image", RemoveLabel: "Delete favorite", Width: 66,
				OnRemove: func() {},
				Preview:  func() { ui.Text(c, "🌿").PassThrough() },
			})
		})
	case "settings-data":
		ui.Column(c).Width(330).Gap(8).Children(func() {
			kit.SettingsDataPath(c, "Data storage path", "C:/Example/Documents/Clipboard/profile.db", func() {
				kit.SettingsDataAction(c, "Change", "", false)
				kit.SettingsDataAction(c, "Open", "", false)
			})
			kit.SettingsDataActions(c, func() {
				kit.SettingsDataAction(c, "Export data", "export", false)
				kit.SettingsDataAction(c, "Import data", "import", false)
			})
		})
	case "tag-toolbar":
		kit.TagToolbar(c, func() {
			kit.SettingsLabel(c, "Work").Grow(1)
		}, []uikit.HeaderAction{
			{Icon: "collapse", Label: "Compact view"}, {Icon: "reveal", Label: "Reveal entries", Active: true},
			{Icon: "manage", Label: "Manage"}, {Icon: "add", Label: "Add entry"}, {Icon: "more", Label: "Tag actions"},
		}, nil)
	case "settings-remove":
		ui.Column(c).Width(330).Gap(8).Children(func() {
			ui.Row(c).FillWidth().AlignItems(ui.Center).Children(func() {
				kit.SettingsLabel(c, "Custom background").Grow(1)
				kit.SettingsRemoveButton(c, "Remove background", true, false)
			})
			ui.Row(c).FillWidth().AlignItems(ui.Center).Children(func() {
				kit.SettingsLabel(c, "Custom theme").Grow(1)
				kit.SettingsRemoveButton(c, "Remove theme", false, false)
			})
		})
	default:
		return false
	}
	return true
}
