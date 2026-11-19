package uikit

import "github.com/egoist/mygo/ui"

type HistoryCardOptions struct {
	Source   string
	Time     string
	Summary  string
	Selected bool
	Compact  bool
	Pinned   bool
	Actions  []HeaderAction
	Body     func()
	Activate func()
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
				text(c, options.Summary, content).Padding(content.Padding[:]...).Wrap()
			}
		})
	})
	if options.Activate != nil && card.Clicked() {
		options.Activate()
	}
	return card
}

// HistoryCardMeta renders the source label, available actions and timestamp.
func (k Kit) HistoryCardMeta(c *ui.Context, options HistoryCardMetaOptions) {
	meta := k.style("historyMeta")
	paint(ui.Row(c).Height(meta.Height).Gap(8).AlignItems(ui.Center), meta).Children(func() {
		if options.Pinned {
			ui.Icon(c, headerIcons["pin"]).Size(10, 10).TextColor(c.Theme().Accent)
		}
		text(c, options.Source, k.style("historySource")).Grow(1)
		ui.Row(c).Gap(4).AlignItems(ui.Center).Children(func() {
			for _, action := range options.Actions {
				k.historyAction(c, action)
			}
		})
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
