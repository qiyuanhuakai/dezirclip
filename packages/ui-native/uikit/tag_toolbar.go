package uikit

import (
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
)

// TagPicker provides keyboard and text filtering in a compact toolbar.
func (k Kit) TagPicker(c *ui.Context, value *string, label, placeholder string, choices []string, disabled bool) (ui.Element, bool) {
	return k.combo(c, value, label, placeholder, choices, disabled, 27)
}

func (k Kit) TagToolbar(c *ui.Context, leading func(), actions []HeaderAction, trailing func()) {
	row := k.style("tagToolbar")
	paint(ui.Row(c).FillWidth().MinHeight(row.Height).Gap(row.ColumnGap).AlignItems(ui.Center), row).Children(func() {
		leading()
		for _, action := range actions {
			button := k.TagToolbarButton(c, action)
			if action.Run != nil && button.Clicked() {
				action.Run()
			}
		}
		if trailing != nil {
			trailing()
		}
	})
}

// TagToolbarTitle shows the current tag in the wide layout, where the rail owns selection.
func (k Kit) TagToolbarTitle(c *ui.Context, label string) {
	style, iconStyle := k.style("tagToolbarTitle"), k.style("tagToolbarTitleIcon")
	ui.Row(c).Grow(1).MinWidth(0).Gap(5).AlignItems(ui.Center).Tooltip(label).Children(func() {
		ui.Icon(c, headerIcons["tag"]).Size(iconStyle.Width, iconStyle.Height).
			TextColor(styleColor(iconStyle.Text)).Shrink(0).PassThrough()
		text(c, label, style).MinWidth(0).Shrink(1).SingleLine().PassThrough()
	})
}

type TagBatchToolbarOptions struct {
	SelectLabel, CountLabel, DeleteLabel        string
	AllSelected, SelectDisabled, DeleteDisabled bool
	OnSelect, OnDelete                          func()
}

// TagBatchToolbar separates the selection state from the destructive action.
func (k Kit) TagBatchToolbar(c *ui.Context, options TagBatchToolbarOptions) {
	style := k.style("tagBatchToolbar")
	row := ui.Row(c).FillWidth().Wrap().GapX(style.ColumnGap).GapY(style.RowGap).AlignItems(ui.Center)
	paint(row, style).Children(func() {
		if k.tagBatchButton(c, "tagBatchSelect", options.SelectLabel, options.AllSelected, options.SelectDisabled).Clicked() && options.OnSelect != nil {
			options.OnSelect()
		}
		text(c, options.CountLabel, k.style("tagBatchCount")).Grow(1).MinWidth(56).SingleLine().Role(ui.RoleStatus)
		if k.tagBatchButton(c, "tagBatchDelete", options.DeleteLabel, false, options.DeleteDisabled).Clicked() && options.OnDelete != nil {
			options.OnDelete()
		}
	})
}

func (k Kit) tagBatchButton(c *ui.Context, widget, label string, selected, disabled bool) ui.Element {
	style := k.style(widget)
	button := ui.ButtonBase(c).Label(label).Height(style.Height).Shrink(0).
		Selected(selected).Disabled(disabled).Center().FocusRing(true)
	paint(button, k.interactive(c, button, style, false)).Children(func() {
		text(c, label, style).SingleLine().PassThrough()
	})
	return button
}

func (k Kit) TagToolbarButton(c *ui.Context, action HeaderAction) ui.Element {
	style := k.style("tagToolbarButton")
	if action.Active {
		style = k.style("tagToolbarButtonActive")
	}
	button := ui.ButtonBase(c).Label(action.Label).Tooltip(action.Label).Size(style.Width, style.Height).
		Selected(action.Active).Center().Shrink(0).Disabled(action.Disabled).FocusRing(true).
		Translate(style.TranslateX, style.TranslateY)
	paint(button, k.interactive(c, button, style, action.Active)).Children(func() {
		if symbol := headerIcons[action.Icon]; symbol != nil {
			ui.Icon(c, symbol).Size(14, 14).TextColor(styleColor(style.Text)).PassThrough()
		}
	})
	return button
}

type TagSearchVariant string

const (
	TagContentsSearch TagSearchVariant = "tagSearch"
	TagRailSearch     TagSearchVariant = "tagRailSearch"
	TagPickerSearch   TagSearchVariant = "tagPickerSearch"
)

type TagSearchOptions struct {
	Variant           TagSearchVariant
	Query             *string
	Label, ClearLabel string
	Disabled          bool
	OnKey             func(ui.TextKey) bool
}

func (k Kit) TagSearch(c *ui.Context, options TagSearchOptions) ui.Element {
	if options.Query == nil {
		return ui.Element{}
	}
	var input ui.Element
	container := ui.Column(c).FillWidth().MinWidth(0)
	if options.Variant == TagContentsSearch || options.Variant == "" {
		options.Variant = TagContentsSearch
		container.Padding(2, 7, 6, 3)
	}
	container.Children(func() {
		input = k.searchField(c, HeaderOptions{
			Query: options.Query, Placeholder: options.Label, OnQueryKey: options.OnKey,
			QueryPopup: func(field ui.Element) {
				if options.Disabled || options.ClearLabel == "" || *options.Query == "" {
					return
				}
				style := k.style("tagSearchClear")
				button := ui.ButtonBase(c).Absolute().Right(4).Top(4).Size(style.Width, style.Height).
					Label(options.ClearLabel).Center()
				paint(button, k.interactive(c, button, style, false)).Children(func() {
					size := k.style("tagSearchClearIcon")
					ui.Icon(c, closeIcon).Size(size.Width, size.Height).TextColor(styleColor(style.Text)).PassThrough()
				})
				if button.Clicked() {
					*options.Query = ""
					field.Focus()
				}
			},
		}, string(options.Variant)).Disabled(options.Disabled)
	})
	return input
}

type TagChoice struct {
	ID, Label string
	Count     uint64
}

type TagNavigationOptions struct {
	Label, SearchLabel, CreateLabel, EmptyLabel string
	Selected                                    string
	Choices                                     []TagChoice
	OnChoose                                    func(string)
	OnCreate                                    func()
}

func (k Kit) TagNavigation(c *ui.Context, options TagNavigationOptions) (ui.Element, bool) {
	current := options.Label
	for _, choice := range options.Choices {
		if choice.ID == options.Selected {
			current = choice.Label
			break
		}
	}
	trigger := ui.ButtonBase(c).Height(27).Grow(1).MinWidth(72).AlignItems(ui.Center).Gap(5).
		Label(options.Label + ": " + current).Tooltip(current)
	type pickerState struct {
		open, focus          bool
		query, previousQuery string
		active               int
	}
	state := ui.Local(trigger, "tag-navigation", func() pickerState { return pickerState{} })
	if trigger.Clicked() || !state.open && (trigger.Shortcut(0, ui.KeyDown) || trigger.Shortcut(0, ui.KeyUp)) {
		if state.open {
			state.open = false
		} else {
			*state = pickerState{open: true, focus: true}
			for index, choice := range options.Choices {
				if choice.ID == options.Selected {
					state.active = index
					break
				}
			}
		}
	}
	style := k.style("tagPickerTrigger")
	paint(trigger.Selected(state.open), k.interactive(c, trigger, style, state.open)).Children(func() {
		ui.Icon(c, headerIcons["tag"]).Size(14, 14).Shrink(0).TextColor(styleColor(style.Text)).PassThrough()
		text(c, current, style).Grow(1).MinWidth(0).SingleLine().PassThrough()
		ui.Icon(c, chevronDown).Size(13, 13).Shrink(0).TextColor(styleColor(style.Text)).PassThrough()
	})
	width, height := c.Size()
	ui.PopoverBase(c, trigger, &state.open, func(panel ui.Element) {
		surface := k.style("contextMenu")
		paint(panel.Width(min(420, max(0, width-12))).Translate(0, 3).Gap(6), surface)
		choose := func(id string) {
			state.open = false
			trigger.Focus()
			if options.OnChoose != nil {
				options.OnChoose(id)
			}
		}
		var input ui.Element
		ui.Row(c).FillWidth().Gap(5).AlignItems(ui.Center).Children(func() {
			ui.Column(c).Width(0).Grow(1).MinWidth(0).Children(func() {
				input = k.TagSearch(c, TagSearchOptions{Variant: TagPickerSearch,
					Query: &state.query, Label: options.SearchLabel})
			})
			if options.OnCreate != nil {
				if k.TagToolbarButton(c, HeaderAction{Icon: "add", Label: options.CreateLabel}).Clicked() {
					state.open = false
					options.OnCreate()
				}
			}
		})
		if state.focus {
			input.Focus()
			state.focus = false
		}
		if state.query != state.previousQuery {
			state.previousQuery, state.active = state.query, 0
		}
		matches := make([]TagChoice, 0, len(options.Choices))
		for _, choice := range options.Choices {
			if MatchesTagName(choice.Label, state.query) {
				matches = append(matches, choice)
			}
		}
		if len(matches) == 0 {
			text(c, options.EmptyLabel, k.style("settingsLabel")).Wrap()
			return
		}
		state.active = min(state.active, len(matches)-1)
		moved := false
		if !input.Composing() {
			before := state.active
			if input.Shortcut(0, ui.KeyDown) {
				state.active = (state.active + 1) % len(matches)
			}
			if input.Shortcut(0, ui.KeyUp) {
				state.active = (state.active + len(matches) - 1) % len(matches)
			}
			moved = state.active != before
		}
		if input.Submitted() {
			choose(matches[state.active].ID)
			return
		}
		ui.Scroll(c).FillWidth().MaxHeight(max(38, min(height-165, 228))).Children(func() {
			ui.Column(c).FillWidth().Children(func() {
				for i, choice := range matches {
					item := k.tagChoice(c, choice, choice.ID == options.Selected, i == state.active, false, true)
					if moved && i == state.active {
						item.ScrollIntoView()
					}
					if item.Clicked() {
						choose(choice.ID)
					}
				}
			})
		})
	})
	return trigger, state.open
}

func (k Kit) TagRailChoice(c *ui.Context, choice TagChoice, selected, collapsed bool) ui.Element {
	return k.tagChoice(c, choice, selected, false, collapsed, false)
}

func (k Kit) tagChoice(c *ui.Context, choice TagChoice, selected, active, collapsed, popup bool) ui.Element {
	widget := "settingsRailItem"
	if selected {
		widget = "settingsRailActive"
	}
	if popup {
		widget = "contextMenuItem"
		if selected || active {
			widget = "contextMenuItemActive"
		}
	}
	style := k.style(widget)
	button := ui.ButtonBase(c).FillWidth().Height(34).Label(choice.Label).Tooltip(choice.Label).
		Selected(selected).AlignItems(ui.Center).Gap(4)
	paint(button, k.interactive(c, button, style, selected)).Children(func() {
		if collapsed {
			for _, initial := range choice.Label {
				button.Center()
				text(c, string(initial), style).SingleLine().PassThrough()
				break
			}
			return
		}
		text(c, choice.Label, style).Grow(1).MinWidth(0).SingleLine().PassThrough()
		style.Size = 10
		text(c, strconv.FormatUint(choice.Count, 10), style).SingleLine().Opacity(.65).PassThrough()
	})
	return button
}

func MatchesTagName(name, query string) bool {
	query, _ = strings.CutPrefix(strings.ToLower(strings.TrimSpace(query)), "tag:")
	return strings.Contains(strings.ToLower(name), strings.TrimSpace(query))
}
