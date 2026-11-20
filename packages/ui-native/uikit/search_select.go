package uikit

import (
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
)

type SearchSelectOptions struct {
	Label, Placeholder, NoMatches string
	Value                         *string
	Choices                       []SelectOption
	Disabled                      bool
}

// SearchSelect displays a committed choice separately from the search query.
// It reports only a choice from the list, using the option's value as its identity.
func (k Kit) SearchSelect(c *ui.Context, options SearchSelectOptions) (ui.Element, bool) {
	style := k.style("select")
	box := ui.Box(c).FillWidth().MinWidth(0).Height(style.Height)
	query := ui.Local(box, "select-query", func() string { return "" })
	changed := false
	box.Children(func() {
		parts := ui.ComboboxBase(c, query)
		input := parts.Input.Fill().Label(options.Label).Disabled(options.Disabled)
		fieldStyle := style
		switch {
		case options.Disabled:
			fieldStyle = k.style("selectDisabled")
		case input.Focused() || parts.Open():
			fieldStyle = k.style("selectFocus")
		case input.Hovered():
			fieldStyle = k.style("selectHover")
		}
		indicator := k.style("selectIndicator")
		end := max(20, indicator.Width) + 4
		styledText(paint(input.FocusRing(false), k.animatePaint(input, fieldStyle)), fieldStyle).
			PaddingStart(9).PaddingEnd(end)
		if *query != "" {
			styledText(input, k.style("selectSearchQuery"))
		} else {
			label := options.Placeholder
			for _, choice := range options.Choices {
				if choice.Value == *options.Value {
					label = choice.Label
					break
				}
			}
			input.Value(label)
			ui.Row(c).Absolute().Left(9).Right(end).Top(0).Bottom(0).AlignItems(ui.Center).PassThrough().
				Children(func() { text(c, label, fieldStyle).Grow(1).MinWidth(0).SingleLine().PassThrough() })
		}
		paint(ui.Row(c).Absolute().Right(3).Top(0).Bottom(0).Width(indicator.Width).Center().PassThrough(), indicator).
			Children(func() {
				ui.Icon(c, chevronDown).Size(20, 20).TextColor(styleColor(indicator.Text)).PassThrough()
			})
		if options.Disabled {
			parts.SetOpen(false)
		}
		parts.Popup(func(panel ui.Element) {
			menu, list := k.style("selectMenu"), k.style("selectMenuList")
			paint(panel.Translate(0, menu.MarginTop), menu)
			_, viewportHeight := c.Size()
			ui.Scroll(c).FillWidth().MaxHeight(max(40, min(300, viewportHeight-64))).Children(func() {
				ui.Column(c).FillWidth().Padding(list.Padding[:]...).Children(func() {
					tokens := strings.Fields(strings.ToLower(*query))
					matches, selectable := 0, 0
					for index, choice := range options.Choices {
						if parts.Filtering() && !matchesSelectOption(choice, tokens) {
							continue
						}
						matches++
						var row ui.Element
						widget := "selectOption"
						if choice.Disabled {
							row = ui.Row(c).FillWidth()
							widget = "selectOptionDisabled"
						} else {
							selectable++
							row = parts.Item(strconv.Itoa(index)).Label(choice.Label).FillWidth()
							switch {
							case choice.Value == *options.Value:
								widget = "selectSelected"
							case row.Highlighted():
								widget = "selectOptionFocused"
							}
						}
						rowStyle := k.style(widget)
						paint(row.MinWidth(0).Height(rowStyle.Height).AlignItems(ui.Center), rowStyle).
							Children(func() { text(c, choice.Label, rowStyle).MinWidth(0).SingleLine().PassThrough() })
					}
					if matches > 0 && selectable == 0 {
						parts.Item("").Disabled(true).Size(0, 0).PassThrough()
					}
					if matches == 0 {
						// ComboboxBase needs one item for the popup to remain visible.
						// Its empty token cannot select a setting.
						empty := k.style("selectOptionDisabled")
						paint(parts.Item("").Label(options.NoMatches).Disabled(true).Height(empty.Height).Center(), empty).
							Children(func() { text(c, options.NoMatches, empty).SingleLine().PassThrough() })
					}
				})
			})
		})
		if token, ok := parts.Chosen(); ok && !options.Disabled {
			if index, err := strconv.Atoi(token); err == nil && index >= 0 && index < len(options.Choices) {
				value := options.Choices[index].Value
				changed = value != *options.Value
				*options.Value = value
			}
		}
		if !parts.Open() {
			*query = ""
		}
	})
	return box, changed
}

func matchesSelectOption(option SelectOption, terms []string) bool {
	haystack := strings.ToLower(option.Label + " " + option.Value)
	for _, term := range terms {
		if !strings.Contains(haystack, term) {
			return false
		}
	}
	return true
}
