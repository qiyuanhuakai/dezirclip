package uikit

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/go-text/typesetting/segmenter"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

type HistoryCardOptions struct {
	Source  string
	Time    string
	Summary string
	// Placeholder describes an unavailable, loading or empty text preview.
	Placeholder string
	Selected    bool
	Compact     bool
	Concealed   bool
	First       bool
	Pinned      bool
	Actions     []HeaderAction
	Tags        []HistoryTag
	Match       string
	TextFont    string
	TextSize    float32
	TextRows    int
	TagSize     float32
	Rich        *HistoryRichText
	Body        func()
	OCRBadge    *HistoryOCRBadgeOptions
	OCRResult   *HistoryOCRResultOptions
	// Media draws using the width remaining after the card's margins, frame and leading controls.
	Media    func(contentWidth float32)
	Width    float32
	Leading  func()
	Activate func()
	Reorder  *HistoryReorder

	FlushHorizontal bool
	ReadOnly        bool
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
var historyFileText = icon("FileText")

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
	OCR     *HistoryOCRBadgeOptions
}

func (k Kit) HistoryCard(c *ui.Context, options HistoryCardOptions) ui.Element {
	// Keep row actions and attachments available to assistive technology.
	var card ui.Element
	if options.ReadOnly {
		card = ui.Column(c)
	} else {
		card = ui.ButtonBase(c)
	}
	label := options.Summary
	if label == "" && !options.Concealed {
		label = options.Placeholder
	}
	card.Role(ui.RoleGroup).Label(label)
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
	if !options.ReadOnly && !options.Selected && card.Hovered() && themedHover {
		style = k.style(hoverStyle)
	}
	if !options.ReadOnly && (card.Pressed() || !options.Selected && card.Hovered() && !themedHover) {
		style = k.interactive(c, card, style, false)
	} else {
		style = k.animatePaint(card, style)
	}
	if options.FlushHorizontal {
		style.Margin[1], style.Margin[3] = 0, 0
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
			if !options.Compact && (options.Source != "" || options.Time != "" || options.Pinned || len(options.Actions) > 0 || options.OCRBadge != nil) {
				k.HistoryCardMeta(c, HistoryCardMetaOptions{
					Source: options.Source, Time: options.Time, Pinned: options.Pinned, Actions: options.Actions, Reorder: options.Reorder,
					OCR: options.OCRBadge,
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
				}
				if options.Media != nil {
					options.Media(max(1, width))
				} else {
					k.historyRichText(c, options, max(1, width))
				}
			} else {
				k.historyCardText(c, options)
			}
			if options.OCRResult != nil {
				k.HistoryOCRResult(c, *options.OCRResult)
			}
			if !options.Compact && len(options.Tags) > 0 {
				k.HistoryCardTags(c, options.Tags, options.TagSize, false, 0)
			}
		}
		if options.Leading != nil || options.Compact && options.Reorder != nil {
			ui.Row(c).Grow(1).Gap(4).AlignItems(ui.Center).Children(func() {
				if options.Leading != nil {
					options.Leading()
				} else {
					k.historyReorderGrip(c, options.Reorder, k.style("historySource"), 22)
				}
				ui.Column(c).Grow(1).MinWidth(0).Children(contentColumn)
			})
		} else {
			ui.Column(c).Grow(1).MinWidth(0).Children(contentColumn)
		}
		if options.Compact && options.Pinned {
			pin := k.style("historyCompactPin")
			paint(ui.Box(c).Absolute().Left(0).Top(0).Bottom(0).Width(pin.Width).PassThrough(), pin)
		}
		if options.Compact && len(options.Tags) > 0 {
			width := options.Width
			if width <= 0 {
				width, _ = c.Size()
			}
			k.HistoryCardTags(c, options.Tags, options.TagSize, true, width-style.Margin[1]-style.Margin[3]-horizontalBorders(style))
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
	if summary == "" && options.Placeholder != "" && !options.Concealed {
		placeholder := k.style("historyPreviewPlaceholder")
		placeholder.Size, placeholder.LineHeight = content.Size, content.LineHeight
		ui.Row(c).FillWidth().MinWidth(0).AlignItems(ui.Center).Gap(placeholder.ColumnGap).
			Padding(content.Padding[:]...).MinHeight(minHeight).Children(func() {
			ui.Icon(c, historyFileText).Size(14, 14).TextColor(styleColor(placeholder.Text)).Shrink(0).PassThrough()
			text(c, options.Placeholder, placeholder).MinWidth(0).Wrap().Role(ui.RoleStatus).PassThrough()
		})
		return
	}
	wrap := historyTextCanWrap(summary)
	drawText := func() {
		var label ui.Element
		if spans := k.matchSpans(summary, options.Match); spans != nil {
			label = styledText(ui.RichText(c, spans...), content)
		} else {
			label = text(c, summary, content)
		}
		label.Padding(content.Padding[:]...).MinHeight(minHeight)
		if wrap {
			label.Wrap()
		} else {
			label.NoWrap().FillWidth().MinWidth(0)
		}
		maxLines := content.MaxLines
		if options.TextRows > 0 {
			maxLines = options.TextRows
		}
		label.MaxLines(maxLines)
		if content.MaxHeight > 0 {
			label.MaxHeight(content.MaxHeight).Clip()
		}
	}
	if wrap {
		drawText()
	} else {
		// Clip the painted glyphs of the child, including any overhang.
		ui.Box(c).FillWidth().MinWidth(0).ClipX().Children(drawText)
	}
}

// A single run with no Unicode line break opportunity clips horizontally in WebView.
func historyTextCanWrap(summary string) bool {
	var boundaries segmenter.Segmenter
	if err := boundaries.InitWithString(summary); err != nil {
		return true
	}
	lines := boundaries.LineIterator()
	return lines.Next() && lines.Next()
}

// HistoryCardTags draws the colored labels on a clipboard entry. In a compact
// card the labels are overlaid at the lower right and do not enlarge the card.
func (k Kit) HistoryCardTags(c *ui.Context, tags []HistoryTag, size float32, compact bool, cardWidth float32) ui.Element {
	group, chip := k.style("historyTags"), k.style("historyTag")
	if compact {
		group, chip = k.style("historyTagsCompact"), k.style("historyTagCompact")
	}
	if size > 0 {
		// The sampled size uses the default 10px preference. Keep the theme offset.
		chip.Size += size - 10
	}
	row := ui.Row(c).Gap(group.ColumnGap).Margin(group.MarginTop, 0, 0).Opacity(group.Opacity).PassThrough()
	if compact {
		row.Absolute().Right(group.Right).Bottom(group.Bottom).MaxWidth(max(0, cardWidth*group.MaxWidthPercent/100)).Clip()
	} else {
		row.FillWidth().Justify(ui.End).Wrap()
	}
	row.Children(func() {
		for _, tag := range tags {
			style := chip
			style.Fill = rgba(tag.Color)
			paint(text(c, tag.Name, style).SingleLine().MinHeight(style.MinHeight), style).Shrink(0).PassThrough()
		}
	})
	return row
}

// HistoryCardMeta renders the source label, available actions and timestamp.
func (k Kit) HistoryCardMeta(c *ui.Context, options HistoryCardMetaOptions) {
	meta := k.style("historyMeta")
	if len(options.Actions) == 0 && options.OCR == nil {
		if compactMeta, ok := k.widgets["historyMetaWithoutActions"]; ok {
			meta = compactMeta
		}
	}
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
		if options.OCR != nil {
			ui.Row(c).Gap(6).AlignItems(ui.Center).Opacity(0.6).Children(func() {
				k.HistoryOCRBadge(c, *options.OCR)
				text(c, options.Time, k.style("historyTime"))
			})
		} else {
			text(c, options.Time, k.style("historyTime")).Opacity(0.6)
		}
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
