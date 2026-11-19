package uikit

import (
	"strings"

	"github.com/egoist/mygo/ui"
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
	Activate func()
}

type HistoryTag struct {
	Name  string
	Color ui.Color
}

type HistoryCardMetaOptions struct {
	Source  string
	Time    string
	Pinned  bool
	Actions []HeaderAction
}

func (k Kit) HistoryCard(c *ui.Context, options HistoryCardOptions) ui.Element {
	style := k.style("historyCard")
	if options.Compact {
		style = k.style("historyCardCompact")
	} else if options.Selected {
		style = k.style("historyCardSelected")
	}
	if options.Compact && options.Selected {
		selected := k.style("historyCardSelected")
		style.Fill, style.Border, style.Outline, style.Shadows = selected.Fill, selected.Border, selected.Outline, selected.Shadows
	}
	card := paint(ui.ButtonBase(c).Label(options.Summary), style).Margin(style.Margin[:]...)
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
		ui.Column(c).Grow(1).Children(func() {
			if options.Source != "" || options.Time != "" || options.Pinned || len(options.Actions) > 0 {
				k.HistoryCardMeta(c, HistoryCardMetaOptions{
					Source: options.Source, Time: options.Time, Pinned: options.Pinned, Actions: options.Actions,
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
		})
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
			if options.Pinned {
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
	button := paint(ui.ButtonBase(c).Label(action.Label).Size(style.Width, style.Height).Disabled(action.Disabled), style)
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
