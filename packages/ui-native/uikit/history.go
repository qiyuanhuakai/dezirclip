package uikit

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

type HistoryCardOptions struct {
	Source    string
	Time      string
	Summary   string
	Selected  bool
	Compact   bool
	Concealed bool
	First     bool
	Pinned    bool
	Actions   []HeaderAction
	Tags      []HistoryTag
	Match     string
	TextFont  string
	TextSize  float32
	TextRows  int
	TagSize   float32
	Rich      *HistoryRichText
	Body      func()
	// Media draws using the width remaining after the card's margins, frame and leading controls.
	Media    func(contentWidth float32)
	Width    float32
	Leading  func()
	Activate func()
	Reorder  *HistoryReorder
}

// HistoryReorder enables a local drag grip and Alt+arrow movement for a pinned card.
type HistoryReorder struct {
	Label    string
	Value    any
	Disabled bool
	Focus    bool
	OnMove   func(earlier bool)
	OnDrag   func()
}

var historyGrip = icon("GripHorizontal")

type HistoryTag struct {
	Name  string
	Color ui.Color
}

type HistoryCardMetaOptions struct {
	Source  string
	Time    string
	Pinned  bool
	Actions []HeaderAction
	Reorder *HistoryReorder
}

func (k Kit) HistoryCard(c *ui.Context, options HistoryCardOptions) ui.Element {
	// Keep row actions and attachments available to assistive technology.
	card := ui.ButtonBase(c).Role(ui.RoleGroup).Label(options.Summary)
	style := k.style("historyCard")
	hoverStyle := "historyCardHover"
	if options.Compact {
		style = k.style("historyCardCompact")
		hoverStyle = "historyCardCompactHover"
		if options.Selected {
			if _, ok := k.widgets["historyCardCompactSelected"]; ok {
				style = k.style("historyCardCompactSelected")
			} else {
				selected := k.style("historyCardSelected")
				style.Fill, style.Border, style.Outline, style.Shadows = selected.Fill, selected.Border, selected.Outline, selected.Shadows
			}
		}
	} else if options.Selected {
		style = k.style("historyCardSelected")
	}
	_, themedHover := k.widgets[hoverStyle]
	if !options.Selected && card.Hovered() && themedHover {
		style = k.style(hoverStyle)
	}
	if card.Pressed() || !options.Selected && card.Hovered() && !themedHover {
		style = k.interactive(c, card, style, false)
	} else {
		style = k.animatePaint(card, style)
	}
	paint(card, style).Margin(style.Margin[:]...)
	if len(options.Tags) > 0 {
		names := make([]string, 0, len(options.Tags))
		for _, tag := range options.Tags {
			names = append(names, tag.Name)
		}
		card.Description(strings.Join(names, ", "))
	}
	card.Children(func() {
		contentColumn := func() {
			if options.Compact {
				// Some themes retain a pixel of metadata padding in compact cards.
				if spacer := k.style("historyMetaCompact").Height; spacer > 0 {
					ui.Box(c).Height(spacer)
				}
			}
			if !options.Compact && (options.Source != "" || options.Time != "" || options.Pinned || len(options.Actions) > 0) {
				k.HistoryCardMeta(c, HistoryCardMetaOptions{
					Source: options.Source, Time: options.Time, Pinned: options.Pinned, Actions: options.Actions, Reorder: options.Reorder,
				})
			}
			if options.Body != nil {
				options.Body()
			} else if options.Media != nil || options.Rich != nil && !options.Concealed {
				width := options.Width
				if width <= 0 {
					width, _ = c.Size()
				}
				width -= style.Margin[1] + style.Margin[3] + style.Padding[1] + style.Padding[3]
				width -= horizontalBorders(style)
				switch {
				case options.Leading != nil:
					width -= 28
				case options.Compact && options.Reorder != nil:
					width -= 20
				case options.Compact && options.Pinned:
					width -= 14
				}
				if options.Media != nil {
					options.Media(max(1, width))
				} else {
					k.historyRichText(c, options, max(1, width))
				}
			} else {
				k.historyCardText(c, options)
			}
			if len(options.Tags) > 0 {
				k.historyTags(c, options.Tags, options.TagSize)
			}
		}
		if options.Leading != nil || options.Compact && (options.Reorder != nil || options.Pinned) {
			ui.Row(c).Grow(1).Gap(4).AlignItems(ui.Center).Children(func() {
				if options.Leading != nil {
					options.Leading()
				} else if options.Reorder != nil {
					k.historyReorderGrip(c, options.Reorder, k.style("historySource"), 22)
				} else {
					ui.Icon(c, headerIcons["pin"]).Size(10, 10).TextColor(c.Theme().TextMuted).PassThrough()
				}
				ui.Column(c).Grow(1).MinWidth(0).Children(contentColumn)
			})
		} else {
			ui.Column(c).Grow(1).MinWidth(0).Children(contentColumn)
		}
		if options.Compact && len(options.Actions) > 0 {
			k.compactHistoryActions(c, card, options)
		}
	})
	if options.Activate != nil && card.Clicked() {
		options.Activate()
	}
	return card
}

func (k Kit) historyCardText(c *ui.Context, options HistoryCardOptions) {
	widget := "historyContent"
	if options.Compact {
		widget = "historyContentCompact"
	}
	content := k.style(widget)
	if options.TextSize > 0 {
		content.Size = options.TextSize
	}
	if options.TextFont != "" {
		content.Font = options.TextFont
	}
	minHeight := content.Size * content.MinHeightEm
	if options.Concealed {
		concealed := k.style("historyConcealed")
		content.Size, content.Text, content.Padding, content.Font = concealed.Size, concealed.Text, concealed.Padding, concealed.Font
	}
	summary := strings.Join(strings.FieldsFunc(options.Summary, func(r rune) bool {
		switch r {
		case ' ', '\t', '\n', '\r', '\f':
			return true
		}
		return false
	}), " ")
	var label ui.Element
	if spans := k.matchSpans(summary, options.Match); spans != nil {
		label = styledText(ui.RichText(c, spans...), content)
	} else {
		label = text(c, summary, content)
	}
	label.Padding(content.Padding[:]...).Wrap().MinHeight(minHeight)
	maxLines := content.MaxLines
	if options.TextRows > 0 {
		maxLines = options.TextRows
	}
	label.MaxLines(maxLines)
	if content.MaxHeight > 0 {
		label.MaxHeight(content.MaxHeight).Clip()
	}
}

func (k Kit) historyTags(c *ui.Context, tags []HistoryTag, size float32) {
	if size <= 0 {
		size = 10
	}
	ui.Row(c).FillWidth().Justify(ui.End).Wrap().Gap(4).Margin(2, 0, 0).Children(func() {
		for _, tag := range tags {
			ui.Text(c, tag.Name).FontSize(size).TextColor(ui.RGB(255, 255, 255)).SingleLine().
				Height(size+6).Padding(2, 5).Background(tag.Color).Radius(min(5, c.Theme().Radius))
		}
	})
}

// HistoryCardMeta renders the source label, available actions and timestamp.
func (k Kit) HistoryCardMeta(c *ui.Context, options HistoryCardMetaOptions) {
	meta := k.style("historyMeta")
	paint(ui.Row(c).Height(meta.Height).Margin(0, 0, meta.MarginBottom).Gap(8).AlignItems(ui.Center), meta).Children(func() {
		source := k.style("historySource")
		ui.Row(c).Grow(1).Gap(source.ColumnGap).AlignItems(ui.Center).Children(func() {
			if drag := options.Reorder; drag != nil {
				k.historyReorderGrip(c, drag, source, meta.Height)
			} else if options.Pinned {
				ui.Icon(c, headerIcons["pin"]).Size(10, 10).TextColor(styleColor(source.Text))
			}
			text(c, options.Source, source).SingleLine()
		})
		if len(options.Actions) > 0 {
			k.HistoryCardActions(c, options.Actions, false)
		}
		text(c, options.Time, k.style("historyTime")).Opacity(0.6)
	})
}

func (k Kit) historyReorderGrip(c *ui.Context, drag *HistoryReorder, source theme.WidgetStyle, height float32) {
	grip := ui.ButtonBase(c).Label(drag.Label).Tooltip(drag.Label).Size(16, height).
		Center().Cursor(ui.CursorGrab).Disabled(drag.Disabled).Drag(drag.Value)
	if drag.Focus && !drag.Disabled {
		grip.Focus()
	}
	if grip.Dragging() {
		grip.Cursor(ui.CursorGrabbing)
		if drag.OnDrag != nil {
			drag.OnDrag()
		}
	}
	grip.Children(func() {
		ui.Icon(c, historyGrip).Size(14, 14).Rotate(90).TextColor(styleColor(source.Text)).PassThrough()
	})
	if drag.OnMove != nil {
		if grip.Shortcut(ui.Alt, ui.KeyUp) {
			drag.OnMove(true)
		}
		if grip.Shortcut(ui.Alt, ui.KeyDown) {
			drag.OnMove(false)
		}
	}
}
