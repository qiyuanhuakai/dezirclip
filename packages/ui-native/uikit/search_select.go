package uikit

import (
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/searchtext"
	"golang.org/x/text/unicode/norm"
)

const searchSelectChoicePrefix = "choice:"

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
	wasOpen := ui.Local(box, "select-open", func() bool { return false })
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
					if choice.FontFamily != "" {
						fieldStyle.Font = choice.FontFamily
					}
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
		} else if !*wasOpen && !parts.Open() && input.Focused() && input.Submitted() {
			parts.SetOpen(true)
		}
		hasHighlight := false
		parts.Popup(func(panel ui.Element) {
			menu, list := k.style("selectMenu"), k.style("selectMenuList")
			_, viewportHeight := c.Size()
			anchor := input.Bounds()
			maxHeight := min(300, max(0, viewportHeight-64))
			if anchor.W > 0 && anchor.H > 0 {
				panel.Width(anchor.W)
				room := max(anchor.Y, viewportHeight-anchor.Y-anchor.H)
				border := 2 * menu.BorderWidth
				if menu.BorderWidths != nil {
					border = menu.BorderWidths[0] + menu.BorderWidths[2]
				}
				// Leave the same gap when the toolkit flips the menu above the field.
				maxHeight = min(300, max(0, room-menu.MarginTop-4-border-menu.Padding[0]-menu.Padding[2]))
			}
			paint(panel.Margin(menu.MarginTop, 0, 0), menu)
			ui.Scroll(c).FillWidth().MaxHeight(maxHeight).Children(func() {
				ui.Column(c).FillWidth().Padding(list.Padding[:]...).Children(func() {
					choices := options.Choices
					if parts.Filtering() {
						choices = filterSearchChoices(choices, *query)
					}
					selectable := 0
					var selectedRow ui.Element
					hasSelection := false
					for _, choice := range choices {
						var row ui.Element
						widget := "selectOption"
						if choice.Disabled {
							row = ui.Row(c).FillWidth()
							widget = "selectOptionDisabled"
						} else {
							selectable++
							row = parts.Item(searchSelectChoicePrefix + choice.Value).Label(choice.Label).
								FillWidth().Selected(choice.Value == *options.Value)
							hasHighlight = hasHighlight || row.Highlighted()
							switch {
							case choice.Value == *options.Value:
								widget = "selectSelected"
								selectedRow, hasSelection = row, true
							case row.Highlighted():
								widget = "selectOptionFocused"
							}
						}
						rowStyle := k.style(widget)
						if choice.FontFamily != "" {
							rowStyle.Font = choice.FontFamily
						}
						paint(row.MinWidth(0).MinHeight(rowStyle.Height).AlignItems(ui.Center), rowStyle).
							Children(func() { text(c, choice.Label, rowStyle).MinWidth(0).Grow(1).Wrap().PassThrough() })
					}
					if !*wasOpen && !parts.Filtering() && hasSelection && !hasHighlight {
						selectedRow.ScrollIntoView()
					}
					if len(choices) > 0 && selectable == 0 {
						parts.Item("").Disabled(true).Size(0, 0).PassThrough()
					}
					if len(choices) == 0 {
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
			if value, found := strings.CutPrefix(token, searchSelectChoicePrefix); found {
				for _, choice := range options.Choices {
					if choice.Value == value && !choice.Disabled {
						changed = value != *options.Value
						*options.Value = value
						break
					}
				}
			}
		} else if *wasOpen && parts.Open() && !parts.Filtering() && !hasHighlight && input.Submitted() {
			parts.SetOpen(false)
		}
		if !parts.Open() {
			*query = ""
		}
		*wasOpen = parts.Open()
	})
	return box, changed
}

func filterSearchChoices(choices []SelectOption, query string) []SelectOption {
	terms := strings.Fields(searchSelectText(query))
	if len(terms) == 0 {
		return choices
	}
	var matches []SelectOption
	for _, choice := range choices {
		if matchesSelectOption(choice, terms, false) {
			matches = append(matches, choice)
		}
	}
	if len(matches) == 0 {
		for _, choice := range choices {
			if matchesSelectOption(choice, terms, true) {
				matches = append(matches, choice)
			}
		}
	}
	return matches
}

func searchSelectText(text string) string {
	return strings.Map(func(char rune) rune {
		if char >= '\u0300' && char <= '\u036f' {
			return -1
		}
		return char
	}, norm.NFD.String(strings.ToLower(text)))
}

func matchesSelectOption(option SelectOption, terms []string, allowTypo bool) bool {
	haystack := searchSelectText(option.Label + " " + option.Value)
	missing := ""
	for _, term := range terms {
		if strings.Contains(haystack, term) {
			continue
		}
		if !allowTypo || missing != "" || utf8.RuneCountInString(term) < 4 {
			return false
		}
		missing = term
	}
	if missing == "" {
		return true
	}
	for _, word := range strings.FieldsFunc(haystack, func(char rune) bool {
		return !unicode.IsLetter(char) && !unicode.IsNumber(char)
	}) {
		if searchtext.WithinOneEdit(missing, word) {
			return true
		}
	}
	return false
}
