package uikit

import (
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

// QuickPastePanel keeps the theme's tint legible across the window opacity range.
func (k Kit) QuickPastePanel(c *ui.Context, background ui.Color) ui.Element {
	background.A = uint8(0.82*255 + 0.18*float32(background.A) + 0.5)
	return k.FloatingPanel(c, background, 12, 8)
}

// QuickPasteSearch draws the panel's query field while leaving text events to the caller.
func (k Kit) QuickPasteSearch(c *ui.Context, value *string, label string) ui.Element {
	field := ui.TextInputBase(c, value).FillWidth().Height(36).Label(label).Placeholder(label)
	palette := c.Theme()
	border := palette.Border
	foreground := palette.Text
	if *value == "" && !field.Composing() {
		foreground = palette.TextMuted
	}
	if field.Focused() {
		border = palette.Accent
	}
	return field.Padding(0, 12).Radius(k.style("tag").Radius).
		Background(palette.Surface).Border(1, border).TextColor(foreground).FontSize(14)
}

// QuickPasteRow paints a result and its source using the panel's hover and selection color.
func (k Kit) QuickPasteRow(c *ui.Context, label, source string, selected bool) ui.Element {
	palette := c.Theme()
	row := ui.Column(c).FillWidth().Height(48).Gap(2).Selected(selected).Cursor(ui.CursorPointer)
	style := theme.WidgetStyle{Padding: [4]float32{6, 12, 6, 12}, Radius: min(8, k.style("tag").Radius), Opacity: 1}
	if selected || row.Hovered() {
		style.Fill = rgba(ui.Hex(k.palette().Selected))
	}
	paint(row, k.animatePaint(row, style)).Children(func() {
		ui.Text(c, label).FontSize(13).TextColor(palette.Text).SingleLine()
		ui.Text(c, source).FontSize(11).TextColor(palette.TextMuted).SingleLine()
	})
	return row
}
