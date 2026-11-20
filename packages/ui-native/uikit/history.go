package uikit

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

type HistoryCardOptions struct {
	Source   string
	Time     string
	Summary  string
	Selected bool
	Compact  bool
	Pinned   bool
	Actions  []HeaderAction
	Tags     []HistoryTag
	Match    string
	TextFont string
	TextSize float32
	TextRows int
	TagSize  float32
	Body     func()
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
	if options.Body == nil {
		card.MinHeight(style.Height)
	}
	card.Children(func() {
		contentColumn := func() {
			if !options.Compact && (options.Source != "" || options.Time != "" || options.Pinned || len(options.Actions) > 0) {
				k.HistoryCardMeta(c, HistoryCardMetaOptions{
					Source: options.Source, Time: options.Time, Pinned: options.Pinned, Actions: options.Actions, Reorder: options.Reorder,
				})
			}
			if options.Body != nil {
				options.Body()
			} else {
				content := k.style("historyContent")
				if options.TextSize > 0 {
					content.Size = options.TextSize
				}
				if options.TextFont != "" {
					content.Font = options.TextFont
				}
				var summary ui.Element
				if spans := k.matchSpans(options.Summary, options.Match); spans != nil {
					summary = styledText(ui.RichText(c, spans...), content)
				} else {
					summary = text(c, options.Summary, content)
				}
				summary.Padding(content.Padding[:]...).Wrap()
				if options.TextRows > 0 {
					summary.MaxLines(options.TextRows)
				}
			}
			if len(options.Tags) > 0 {
				k.historyTags(c, options.Tags, options.TagSize)
			}
			if options.Compact && len(options.Actions) > 0 && (card.Hovered() || card.FocusVisible() || options.Selected) {
				ui.Row(c).Absolute().Right(0).Top(0).Gap(4).Children(func() {
					for _, action := range options.Actions {
						k.historyAction(c, action)
					}
				})
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
			ui.Column(c).Grow(1).Children(contentColumn)
		}
	})
	if options.Activate != nil && card.Clicked() {
		options.Activate()
	}
	return card
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
	paint(ui.Row(c).Height(meta.Height).Gap(8).AlignItems(ui.Center), meta).Children(func() {
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
			actions := k.style("historyActions")
			paint(ui.Row(c).Gap(actions.ColumnGap).AlignItems(ui.Center), actions).Children(func() {
				for _, action := range options.Actions {
					k.historyAction(c, action)
				}
			})
		}
		text(c, options.Time, k.style("historyTime")).Opacity(0.6)
	})
}

func (k Kit) historyAction(c *ui.Context, action HeaderAction) {
	style := k.style("historyAction")
	if action.Active {
		style.Fill, style.Border = rgba(c.Theme().Selection), rgba(c.Theme().Accent)
	}
	button := ui.ButtonBase(c).Label(action.Label).Tooltip(action.Label).Size(style.Width, style.Height).Disabled(action.Disabled)
	style = k.interactive(c, button, style, action.Active)
	paint(button, style)
	button.Children(func() {
		if image := headerIcons[action.Icon]; image != nil {
			ui.Icon(c, image).Size(12, 12).TextColor(styleColor(style.Text))
		} else {
			text(c, action.Label, style)
		}
	})
	if action.Run != nil && button.Clicked() {
		action.Run()
	}
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
