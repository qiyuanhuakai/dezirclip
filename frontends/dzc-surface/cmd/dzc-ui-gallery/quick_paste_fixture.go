package main

import (
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-presets"
)

func (g *gallery) quickPasteFixture(c *ui.Context, preset presets.Preset) {
	opacity, _ := strconv.Atoi(g.state)
	kit := preset.Kit()
	kit.Apply(c)
	palette := *c.Theme()
	palette.Background = ui.Hex("#6c6c6c")
	c.SetTheme(&palette)
	kit.QuickPastePanel(c, preset.WindowBackground(100).Alpha(float32(opacity)/100)).Children(func() {
		kit.QuickPasteSearch(c, &g.query, "Search clipboard…")
		ui.Column(c).FillWidth().Children(func() {
			kit.QuickPasteRow(c, "Release checklist", "Editor", true)
			kit.QuickPasteRow(c, "Design notes", "Browser", false)
			kit.QuickPasteRow(c, "Preview package ready for testing", "Terminal", false)
		})
	})
}
