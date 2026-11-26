package uikit

import "github.com/egoist/mygo/ui"

// FloatingPanel keeps all overlays single-layer and paints the chosen theme's
// tint and edge independently from the main window.
func (k Kit) FloatingPanel(c *ui.Context, shade ui.Color, padding, gap float32) ui.Element {
	panel := ui.Column(c).Fill().Padding(padding).Gap(gap)
	radius := max(12, c.Theme().Radius)
	border := c.Theme().Border.Alpha(.6)
	var top, bottom string
	dark := k.Mode == "dark"
	switch k.floatingTheme {
	case "mica":
		radius, top, bottom = 8, "#f2f5fb", "#e0e7f2"
		border = ui.Hex("#a0aac0").Alpha(.50)
		if dark {
			top, bottom, border = "#2d3442", "#1f2430", ui.Hex("#738099").Alpha(.55)
		}
	case "acrylic":
		radius, top, bottom = 12, "#fcfcfe", "#eaeaf3"
		border = ui.Hex("#b9b8cc").Alpha(.65)
		if dark {
			top, bottom, border = "#373742", "#272732", ui.Hex("#a7a6bd").Alpha(.42)
		}
	case "macos":
		radius, top, bottom = 13, "#fafafa", "#fafafa"
		border = ui.Hex("#d4d4d7").Alpha(.72)
		if dark {
			top, bottom, border = "#262628", "#262628", ui.Hex("#555558").Alpha(.78)
		}
	case "liquid-glass":
		radius, top, bottom = 18, "#fffbfd", "#edf7ff"
		border = ui.Hex("#ffffff").Alpha(.85)
		if dark {
			top, bottom, border = "#3a4254", "#191f2d", ui.Hex("#b6c9ed").Alpha(.55)
		}
	case "retro":
		color, edge := "#eeece4", "#393733"
		if dark {
			color, edge = "#252524", "#92918b"
		}
		fill := ui.Hex(color)
		fill.A = shade.A
		return panel.Radius(0).Background(fill).BorderWidth(2, 4, 4, 2).BorderColor(ui.Hex(edge))
	}
	panel = panel.Radius(radius).Border(1, border)
	if top == "" {
		return panel.Background(shade)
	}
	first, last := ui.Hex(top), ui.Hex(bottom)
	first.A, last.A = shade.A, shade.A
	if k.floatingTheme == "acrylic" {
		last.A = uint8(float32(last.A) * .86)
	} else if k.floatingTheme == "liquid-glass" {
		last.A = uint8(float32(last.A) * .75)
	}
	return panel.Gradient(first, last, 180)
}
