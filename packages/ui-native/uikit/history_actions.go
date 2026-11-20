package uikit

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// HistoryCardActions draws inline or compact card actions using the matching theme styles.
func (k Kit) HistoryCardActions(c *ui.Context, actions []HeaderAction, compact bool) ui.Element {
	return k.historyCardActions(c, ui.Row(c), actions, compact, false)
}

func (k Kit) compactHistoryActions(c *ui.Context, card ui.Element, options HistoryCardOptions) {
	anchor := k.style("historyActionsCompactAnchor")
	top := anchor.Top
	if options.First {
		top = max(top, -4)
	}
	row := ui.Row(c).Absolute().Right(anchor.Right).Top(top).PassThrough()
	cardHovered, toolbarHovered := card.Hovered(), row.Hovered()
	visible := cardHovered || toolbarHovered || card.FocusVisible()
	target := float32(0)
	if visible {
		target = 1
	}
	progress := row.Animate("history-toolbar", target, time.Duration(k.palette().TransitionMs)*time.Millisecond)
	if progress <= 0 && !visible {
		row.Invisible()
		return
	}
	hidden := k.style("historyActionsCompact").TranslateY
	k.historyCardActions(c, row, options.Actions, true, !visible).
		Opacity(progress).Translate(0, hidden*(1-progress))
}

func (k Kit) historyCardActions(c *ui.Context, row ui.Element, actions []HeaderAction, compact, fading bool) ui.Element {
	group, widget := "historyActions", "historyAction"
	iconWidget := "historyActionIcon"
	if compact {
		group, widget, iconWidget = "historyActionsCompact", "historyActionCompact", "historyActionCompactIcon"
	}
	style, glyph := k.style(group), k.style(iconWidget)
	style.Opacity = 1
	return paint(row.Gap(style.ColumnGap).AlignItems(ui.Center), style).Children(func() {
		for _, action := range actions {
			style := k.style(widget)
			button := ui.ButtonBase(c).Label(action.Label).Tooltip(action.Label).Size(style.Width, style.Height).
				Disabled(action.Disabled || fading).Center()
			if compact {
				state := widget
				if action.Active {
					state += "Active"
				}
				if button.Hovered() {
					state += "Hover"
				}
				style = k.style(state)
				if !button.Pressed() && !button.FocusVisible() {
					style = k.animatePaint(button, style)
				} else {
					style = k.interactive(c, button, style, action.Active)
				}
			} else {
				if action.Active {
					style.Fill, style.Border = rgba(c.Theme().Selection), rgba(c.Theme().Accent)
				}
				style = k.interactive(c, button, style, action.Active)
			}
			paint(button, style).Translate(style.TranslateX, style.TranslateY)
			button.Children(func() {
				if image := headerIcons[action.Icon]; image != nil {
					ui.Icon(c, image).Size(glyph.Width, glyph.Height).TextColor(styleColor(style.Text)).PassThrough()
				} else {
					text(c, action.Label, style)
				}
			})
			if action.Run != nil && button.Clicked() {
				action.Run()
			}
		}
	})
}
