package uikit

import (
	"strconv"

	"github.com/egoist/mygo/ui"
)

type TagSuggestionsOptions struct {
	Label, Status, Retry string
	Names                []string
	List                 *ui.ListState
	Active               *int
	Color                func(string) ui.Color
	OnSelect             func(string)
	OnRetry              func()
}

// TagSuggestions draws the searchable tag list anchored to the clipboard search field.
func (k Kit) TagSuggestions(c *ui.Context, anchor ui.Element, open *bool, options TagSuggestionsOptions) ui.Element {
	width, height := c.Size()
	return ui.PopoverBase(c, anchor, open, func(panel ui.Element) {
		surface := k.style("tagSuggestions")
		field := anchor.Bounds()
		panel.Width(max(0, width-8))
		if field.W > 0 {
			panel.Width(field.W)
		}
		paint(panel.Margin(surface.MarginTop, 0, 0), surface)
		heading, count := k.style("tagSuggestionHeading"), k.style("tagSuggestionCount")
		paint(ui.Row(c).FillWidth().AlignItems(ui.Center).Gap(6), heading).Children(func() {
			ui.Icon(c, headerIcons["tag"]).Size(13, 13).TextColor(styleColor(heading.Text)).PassThrough()
			text(c, options.Label, heading).Grow(1).SingleLine()
			text(c, strconv.Itoa(len(options.Names)), count).SingleLine()
		})
		if options.Retry != "" {
			style := k.style("tagSuggestionRetry")
			retry := ui.ButtonBase(c).FillWidth().Label(options.Retry).FocusRing(true)
			paint(retry, k.interactive(c, retry, style, false)).Children(func() {
				text(c, options.Retry, style).Wrap().PassThrough()
			})
			if retry.Clicked() && options.OnRetry != nil {
				options.OnRetry()
			}
		}
		if len(options.Names) == 0 {
			if options.Status != "" {
				style := k.style("tagSuggestionStatus")
				paint(ui.Row(c).FillWidth().Center(), style).Children(func() { text(c, options.Status, style).Wrap() })
			}
			return
		}
		rowStyle := k.style("tagSuggestionRow")
		listHeight := min(rowStyle.Height*float32(min(6, len(options.Names))), max(72, height-190))
		options.List.Key = func(index int) any { return options.Names[index] }
		options.List.Label = func(index int) string { return options.Names[index] }
		options.List.Selected = options.Active
		ui.List(c, options.List, len(options.Names)).Height(listHeight).FillWidth().Label(options.Label).Rows(func(cell ui.ListRow) {
			name := options.Names[cell.Index]
			paint(ui.Column(cell.Context).FillWidth().Height(rowStyle.Height), rowStyle).Children(func() {
				button := ui.ButtonBase(cell.Context).FillWidth().MinWidth(0).Label(name).
					Selected(cell.Selected()).AlignItems(ui.Center).FocusRing(true)
				style := k.style("tagSuggestionOption")
				if cell.Selected() || button.Hovered() || button.FocusVisible() {
					style = k.style("tagSuggestionOptionActive")
				}
				style = k.animatePaint(button, style)
				paint(button.Height(style.Height).Gap(style.ColumnGap), style).Children(func() {
					dot := k.style("tagSuggestionDot")
					dot.Fill = rgba(c.Theme().Accent)
					if options.Color != nil {
						dot.Fill = rgba(options.Color(name))
					}
					paint(ui.Box(cell.Context).Size(dot.Width, dot.Height).Shrink(0).PassThrough(), dot)
					text(cell.Context, name, style).Grow(1).MinWidth(0).SingleLine().PassThrough()
				})
				if button.Clicked() && options.OnSelect != nil {
					options.OnSelect(name)
				}
			})
		})
	})
}
