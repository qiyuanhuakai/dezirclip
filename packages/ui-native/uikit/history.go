package uikit

import "github.com/egoist/mygo/ui"

type HistoryCardOptions struct {
	Source   string
	Time     string
	Summary  string
	Selected bool
	Pinned   bool
	Actions  []HeaderAction
	Body     func()
	Activate func()
}

func (k Kit) HistoryCard(c *ui.Context, options HistoryCardOptions) ui.Element {
	style := k.style("historyCard")
	if options.Selected {
		style = k.style("historyCardSelected")
	}
	card := paint(ui.ButtonBase(c).Label(options.Summary).MinHeight(style.Height), style).Margin(style.Margin[:]...)
	card.Children(func() {
		ui.Column(c).Grow(1).Children(func() {
			meta := k.style("historyMeta")
			paint(ui.Row(c).Height(meta.Height).Gap(8).AlignItems(ui.Center), meta).Children(func() {
				source := k.style("historySource")
				if options.Pinned {
					ui.Icon(c, headerIcons["pin"]).Size(10, 10).TextColor(c.Theme().Accent)
				}
				text(c, options.Source, source).Grow(1)
				ui.Row(c).Gap(4).AlignItems(ui.Center).Children(func() {
					for _, action := range options.Actions {
						k.historyAction(c, action)
					}
				})
				text(c, options.Time, k.style("historyTime")).Opacity(0.6)
			})
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
