package main

import (
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

func (g *gallery) themeManagerFixture(c *ui.Context, kit uikit.Kit) {
	width := float32(330)
	if g.state == "wide" {
		width = 500
	}
	active := "macos"
	if g.state == "imported" {
		active = "custom:studio"
	}
	ui.Column(c).Width(width).Children(func() {
		kit.SettingsThemeManager(c, width, uikit.ThemeManagerOptions{
			Label: "Visual theme", BuiltinLabel: "Built-in themes", ImportedLabel: "Imported themes", Active: active,
			Builtins: []uikit.SelectOption{
				{Value: "retro", Label: "Retro"}, {Value: "mica", Label: "Mica"}, {Value: "acrylic", Label: "Acrylic"},
				{Value: "liquid-glass", Label: "Liquid Glass"}, {Value: "macos", Label: "macOS"}, {Value: "scifi", Label: "Sci-Fi"},
			},
			Imported:    []uikit.SelectOption{{Value: "custom:studio", Label: "Studio theme"}},
			ImportLabel: "Import theme", ExportLabel: "Export theme", RemoveLabel: "Remove theme",
		})
	})
}

func (g *gallery) backgroundSettingsFixture(c *ui.Context, kit uikit.Kit) {
	options := uikit.SettingsBackgroundOptions{Label: "Custom background", ChooseLabel: "Choose background", ClearLabel: "Clear background"}
	if g.state == "saved" {
		options.ChooseLabel = "Change background"
		options.Filename = "Workspace wallpaper.png"
	}
	ui.Column(c).Width(330).Children(func() {
		kit.SettingsBackground(c, options, func() {
			kit.SettingsField(c, uikit.SettingsFieldOptions{Stacked: true, Last: true}).Children(func() {
				ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
					kit.SettingsLabel(c, "Background opacity").Grow(1)
					ui.Text(c, "65%").FontSize(11).TextColor(c.Theme().TextMuted)
				})
				opacity := float64(65)
				kit.Slider(c, &opacity, 0, 100, "Background opacity").Step(1).FillWidth()
			})
		})
	})
}
