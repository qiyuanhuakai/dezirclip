package uikit

import "github.com/egoist/mygo/ui"

var headerIcons = map[string]*ui.SVG{
	"back": icon("ChevronLeft"), "pin": icon("Pin"), "unpin": icon("PinOff"),
	"search": icon("Search"), "settings": icon("Settings"), "emoji": icon("Smile"),
	"tag": icon("Tag"), "clear": icon("Trash2"), "hide": closeIcon,
	"reveal": icon("Eye"), "conceal": icon("EyeOff"),
	"add": icon("Plus"), "edit": icon("Pencil"),
	"transform": icon("CaseSensitive"), "qr": icon("QrCode"),
	"scan": icon("Scan"),
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
	Draggable   bool
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
	QueryPopup  func(ui.Element)
}

func (k Kit) Header(c *ui.Context, options HeaderOptions) {
	style := k.style("header")
	width, _ := c.Size()
	count := len(options.Actions)
	if options.Back != nil {
		count++
	}
	limit := float32(0)
	if count > 0 {
		limit = max(22, (width-style.Padding[1]-style.Padding[3]-60-4*float32(count-1))/float32(count))
	}
	paint(ui.Column(c).Gap(style.RowGap), style).Children(func() {
		top := k.style("headerTop")
		paint(ui.Row(c).FillWidth().Height(top.Height).Gap(8).AlignItems(ui.Center), top).Children(func() {
			if options.Back != nil {
				k.headerAction(c, *options.Back, limit)
			}
			title := k.style("headerTitle")
			dragArea := ui.Row(c).Grow(1).MinWidth(0).FillHeight().AlignItems(ui.Center)
			if options.Draggable {
				dragArea.DragWindow()
			}
			dragArea.Children(func() {
				paint(ui.Box(c).PassThrough(), title).Children(func() {
					text(c, options.Title, title).MaxWidth(320).SingleLine().PassThrough()
				})
			})
			ui.Row(c).Gap(4).AlignItems(ui.Center).Children(func() {
				for _, action := range options.Actions {
					k.headerAction(c, action, limit)
				}
			})
		})
		k.ClipboardSearch(c, options)
	})
}

func (k Kit) headerAction(c *ui.Context, action HeaderAction, limit float32) {
	if k.iconButton(c, action.Icon, action.Label, action.Active, action.Disabled, limit).Clicked() && action.Run != nil {
		action.Run()
	}
}

func (k Kit) IconButton(c *ui.Context, name, label string, active, disabled bool) ui.Element {
	return k.iconButton(c, name, label, active, disabled, 0)
}

func (k Kit) iconButton(c *ui.Context, name, label string, active, disabled bool, limit float32) ui.Element {
	style := k.style("headerButton")
	if active {
		style = k.style("headerButtonActive")
	}
	if limit > 0 && style.Width > limit {
		style.Width = limit
		style.Padding[1], style.Padding[3] = 0, 0
	}
	button := ui.ButtonBase(c).Label(label).Tooltip(label).Size(style.Width, style.Height).Disabled(disabled)
	if disabled {
		paint(button, style)
	} else {
		style = k.interactive(c, button, style, active)
		paint(button, style)
	}
	button.Children(func() {
		if image := headerIcons[name]; image != nil {
			ui.Icon(c, image).Size(16, 16).TextColor(styleColor(style.Text)).PassThrough()
		} else {
			text(c, label, style)
		}
	})
	return button
}

func (k Kit) ScrollToTopButton(c *ui.Context, label string) ui.Element {
	style := k.style("headerButton")
	style.Radius, style.CornerRadii = 16, nil
	button := ui.ButtonBase(c).Size(32, 32).Label(label).Tooltip(label).FocusRing(true)
	style = k.interactive(c, button, style, false)
	return paint(button, style).Draw(func(p *ui.Painter, bounds ui.Rect) {
		x, y := bounds.X+bounds.W/2, bounds.Y+bounds.H/2
		ink := styleColor(style.Text)
		var arrow ui.Path
		arrow.MoveTo(x-5, y-1).LineTo(x, y-6).LineTo(x+5, y-1)
		arrow.MoveTo(x, y-6).LineTo(x, y+6)
		p.StrokePath(&arrow, 1.5, ink)
	})
}

// ClipboardSearch draws the main clipboard search field and its content filters.
func (k Kit) ClipboardSearch(c *ui.Context, options HeaderOptions) {
	if options.Query == nil {
		return
	}
	style := k.style("headerSearch")
	paint(ui.Column(c), style).Margin(style.MarginTop, 0, 0, 0).Children(func() {
		field := k.style("headerInput")
		ui.Box(c).Height(field.Height).Children(func() {
			input := ui.TextInputBase(c, options.Query).Fill().Label(options.Placeholder)
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
			if *options.Query == "" && !input.Composing() {
				placeholder := k.style("headerInputPlaceholder")
				field.Text = placeholder.Text
				ui.Row(c).Absolute().Left(field.Padding[3] + field.BorderWidth).
					Right(field.Padding[1] + field.BorderWidth).Height(field.Height).AlignItems(ui.Center).PassThrough().
					Children(func() { text(c, options.Placeholder, field).SingleLine().PassThrough() })
			}
			icon := k.style("headerInputIcon")
			ui.Icon(c, headerIcons["search"]).Absolute().Left(icon.Left).Top((field.Height-icon.Height)/2).
				Size(icon.Width, icon.Height).TextColor(styleColor(icon.Text)).PassThrough()
			if options.QueryPopup != nil {
				options.QueryPopup(input)
			}
		})
		if len(options.Filters) == 0 || options.Filter == nil {
			return
		}
		filters := k.style("headerFilters")
		paint(ui.ScrollHorizontal(c).FillWidth().Gap(filters.ColumnGap).AlignItems(ui.Center), filters).Children(func() {
			for _, filter := range options.Filters {
				selected := *options.Filter == filter.ID
				style := k.style("headerFilter")
				if selected {
					style = k.style("headerFilterSelected")
				}
				button := ui.ButtonBase(c).Label(filter.Label).Height(style.Height).Selected(selected)
				style = k.interactive(c, button, style, selected)
				paint(button, style).Children(func() { text(c, filter.Label, style) })
				if button.Clicked() {
					if selected {
						*options.Filter = ""
					} else {
						*options.Filter = filter.ID
					}
				}
			}
		})
	})
}
