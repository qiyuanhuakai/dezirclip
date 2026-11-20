package main

import (
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-presets"
)

func (g *gallery) opacityFixture(c *ui.Context, preset presets.Preset) {
	opacity, _ := strconv.Atoi(g.state)
	kit := preset.Kit().WithSurfaceOpacity(opacity)
	kit.Apply(c)
	palette := *c.Theme()
	palette.Background = ui.Hex("#6c6c6c")
	c.SetTheme(&palette)
	padding := preset.ShowcasePadding()
	ui.Column(c).Fill().Background(preset.WindowBackground(opacity)).Children(func() {
		kit.WindowContent(c, func() {
			ui.Row(c).FillWidth().Padding(padding[:]...).Gap(16).AlignItems(ui.Start).Children(func() {
				ui.Column(c).Width(246).Children(func() {
					kit.SettingsSectionCard(c, "Appearance", &g.groupOpen, func() {
						kit.SettingsLabel(c, "Color theme")
						kit.SearchSelect(c, uikit.SearchSelectOptions{Label: "Color mode", NoMatches: "No matches", Value: &g.choice,
							Choices: []uikit.SelectOption{
								{Value: "system", Label: "System"}, {Value: "light", Label: "Light"},
								{Value: "dark", Label: "Dark"}, {Value: "disabled", Label: "Unavailable", Disabled: true},
							},
						})
					})
				})
				ui.Column(c).Grow(1).Children(func() {
					kit.HistoryCard(c, uikit.HistoryCardOptions{Source: "Editor", Time: "12:30", Summary: "Clipboard entry",
						Actions: []uikit.HeaderAction{{Icon: "pin", Label: "Pin"}, {Icon: "tag", Label: "Tags"}, {Icon: "hide", Label: "Delete"}},
					})
				})
			})
		})
	})
}
