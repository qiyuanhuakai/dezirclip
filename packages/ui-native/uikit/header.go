package uikit

import "github.com/egoist/mygo/ui"

var headerIcons = map[string]*ui.SVG{
	"back": icon("ChevronLeft"), "pin": icon("Pin"), "unpin": icon("PinOff"),
	"search": icon("Search"), "settings": icon("Settings"), "emoji": icon("Smile"),
	"tag": icon("Tag"), "clear": icon("Trash2"), "hide": closeIcon,
	"reveal": icon("Eye"), "conceal": icon("EyeOff"),
}

type HeaderAction struct {
	Icon     string
	Label    string
	Active   bool
	Disabled bool
	Run      func()
}

type HeaderFilter struct {
	ID    string
	Label string
}

type HeaderOptions struct {
	Title       string
	Back        *HeaderAction
	Actions     []HeaderAction
	Query       *string
	Placeholder string
	Filters     []HeaderFilter
	Filter      *string
	FocusQuery  *bool
	OnQueryEdit func(composing bool)
	OnQueryKey  func(ui.TextKey) bool
	QueryFocus  func(bool)
}

func (k Kit) Header(c *ui.Context, options HeaderOptions) {
	style := k.style("header")
	paint(ui.Column(c).Gap(style.RowGap), style).Children(func() {
		top := k.style("headerTop")
		paint(ui.Row(c).Height(top.Height).Gap(8).AlignItems(ui.Center), top).Children(func() {
			if options.Back != nil {
				k.headerAction(c, *options.Back)
			}
			title := k.style("headerTitle")
			ui.Row(c).Grow(1).AlignItems(ui.Center).Children(func() {
				paint(ui.Box(c), title).Children(func() {
					text(c, options.Title, title).MaxWidth(320).SingleLine()
				})
			})
			ui.Row(c).Gap(4).AlignItems(ui.Center).Children(func() {
				for _, action := range options.Actions {
					k.headerAction(c, action)
				}
			})
		})
		if options.Query != nil {
			k.headerSearch(c, options)
		}
	})
}

func (k Kit) headerAction(c *ui.Context, action HeaderAction) {
	style := k.style("headerButton")
	if action.Active {
		style = k.style("headerButtonActive")
	}
	button := paint(ui.ButtonBase(c).Label(action.Label).Size(style.Width, style.Height).Disabled(action.Disabled), style)
	button.Children(func() {
		if image := headerIcons[action.Icon]; image != nil {
			ui.Icon(c, image).Size(16, 16).TextColor(styleColor(style.Text))
		} else {
			text(c, action.Label, style)
		}
	})
	if action.Run != nil && button.Clicked() {
		action.Run()
	}
}

func (k Kit) headerSearch(c *ui.Context, options HeaderOptions) {
	style := k.style("headerSearch")
	paint(ui.Column(c), style).Margin(style.MarginTop, 0, 0, 0).Children(func() {
		field := k.style("headerInput")
		ui.Box(c).Height(field.Height).Children(func() {
			input := ui.TextInputBase(c, options.Query).Fill().Label(options.Placeholder).Placeholder(options.Placeholder)
			styledText(paint(input, field), field)
			if options.OnQueryKey != nil {
				input.OnTextKey(options.OnQueryKey)
			}
			if options.QueryFocus != nil {
				options.QueryFocus(input.Focused())
			}
			if options.FocusQuery != nil && *options.FocusQuery {
				input.Focus()
				*options.FocusQuery = false
			}
			if options.OnQueryEdit != nil {
				options.OnQueryEdit(input.Composing())
			}
			ui.Icon(c, headerIcons["search"]).Absolute().Left(10).Top((field.Height-14)/2).Size(14, 14).
				TextColor(c.Theme().TextMuted).PassThrough()
		})
		filters := k.style("headerFilters")
		paint(ui.Row(c).Gap(filters.ColumnGap).AlignItems(ui.Center), filters).Children(func() {
			for _, filter := range options.Filters {
				if k.button(c, filter.Label, "headerFilter").Clicked() && options.Filter != nil {
					if *options.Filter == filter.ID {
						*options.Filter = ""
					} else {
						*options.Filter = filter.ID
					}
				}
			}
		})
	})
}
