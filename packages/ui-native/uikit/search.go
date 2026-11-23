package uikit

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
)

type SearchMode struct {
	ID    string
	Label string
}

type SearchOptions struct {
	Query       *string
	Mode        *string
	Modes       []SearchMode
	Label       string
	Placeholder string
	ClearLabel  string
	Error       string
	Results     int
	Loading     bool
}

func (k Kit) SearchBar(c *ui.Context, options SearchOptions) {
	ui.Column(c).Gap(8).Children(func() {
		field, modeBox := k.style("searchInput"), k.style("searchModes")
		ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Box(c).Grow(1).Height(field.Height).Children(func() {
				input := ui.TextInputBase(c, options.Query).Label(options.Label).Placeholder(options.Placeholder).Fill().FocusRing(false)
				if input.Focused() {
					blue := c.Theme().Accent
					field.Border = [4]uint8{blue.R, blue.G, blue.B, blue.A}
				}
				styledText(paintTextField(input, field), field)
				icon := k.style("searchIcon")
				ui.Icon(c, headerIcons["search"]).Size(icon.Width, icon.Height).TextColor(styleColor(icon.Text)).
					Absolute().Left(10).Top((field.Height - icon.Height) / 2).PassThrough()
				if *options.Query != "" {
					clear := k.style("searchClear")
					button := k.button(c, "×", "searchClear").Label(options.ClearLabel).
						Absolute().Right(8).Top((field.Height - clear.Height) / 2).Width(clear.Width)
					if button.Clicked() {
						*options.Query = ""
					}
				}
			})
			paint(ui.Row(c).Width(modeBox.Width).Height(modeBox.Height).Gap(2).AlignItems(ui.Center), modeBox).
				Children(func() {
					for _, mode := range options.Modes {
						widget := "searchMode"
						if *options.Mode == mode.ID {
							widget = "searchModeSelected"
						}
						if k.searchModeButton(c, mode, widget).Clicked() {
							*options.Mode = mode.ID
						}
					}
				})
		})
		switch {
		case options.Error != "":
			style := k.style("searchError")
			text(c, options.Error, style).Padding(style.Padding[:]...)
		case options.Loading:
			style := k.style("searchSpinner")
			ui.Box(c).Size(style.Width, style.Height).AlignSelf(ui.Center).
				Border(style.BorderWidth, styleColor(style.Border)).Radius(style.Radius).
				Draw(func(p *ui.Painter, r ui.Rect) {
					p.Line(r.X+3, r.Y+2, r.X+r.W-3, r.Y+2, 2, c.Theme().Accent)
				})
		case *options.Query != "" && options.Results > 0:
			style := k.style("searchCount")
			text(c, fmt.Sprintf("找到 %d 条结果", options.Results), style).Padding(style.Padding[:]...)
		}
	})
}

func (k Kit) searchModeButton(c *ui.Context, mode SearchMode, widget string) ui.Element {
	style := k.style(widget)
	button := paint(ui.ButtonBase(c).Label(mode.Label).Height(style.Height).Cursor(ui.CursorPointer), style)
	glyph := mode.Label
	switch mode.ID {
	case "fuzzy":
		glyph = "≈"
	case "regex":
		glyph = ".*"
	}
	caption := strings.TrimSpace(strings.TrimPrefix(mode.Label, glyph))
	button.Children(func() {
		symbol, label := k.style("searchModeIcon"), k.style("searchModeLabel")
		symbol.Text, label.Text = style.Text, style.Text
		ui.Row(c).Gap(style.ColumnGap).AlignItems(ui.Center).Children(func() {
			text(c, glyph, symbol)
			if caption != "" {
				text(c, caption, label).Opacity(label.Opacity)
			}
		})
	})
	return button
}
