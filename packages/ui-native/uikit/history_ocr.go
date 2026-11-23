package uikit

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// HistoryOCRBadgeOptions describes the recognition status next to the card timestamp.
type HistoryOCRBadgeOptions struct {
	Status, Label string
}

// HistoryOCRResultOptions describes the inline disclosure below an image.
// Applications own recognition and may supply a message while loading the text.
type HistoryOCRResultOptions struct {
	Label, ToggleLabel, Text, Message string
	Expanded, Failed                  bool
	OnToggle                          func()
}

func (k Kit) HistoryOCRBadge(c *ui.Context, options HistoryOCRBadgeOptions) {
	widget := "historyOCRBadgeUnsupported"
	switch options.Status {
	case "done":
		widget = "historyOCRBadgeDone"
	case "processing":
		widget = "historyOCRBadgeProcessing"
	case "failed":
		widget = "historyOCRBadgeFailed"
	}
	style := k.style(widget)
	paint(text(c, options.Label, style).SingleLine().Shrink(0), style)
}

func (k Kit) HistoryOCRResult(c *ui.Context, options HistoryOCRResultOptions) {
	section := k.style("historyOCRSection")
	paint(ui.Column(c).FillWidth().Margin(section.MarginTop, 0, 0), section).Children(func() {
		marker := "▶"
		if options.Expanded {
			marker = "▼"
		}
		button := k.style("historyOCRButton")
		toggle := ui.ButtonBase(c).AlignSelf(ui.Start).Height(button.Height).FocusRing(true).
			Label(options.ToggleLabel).Tooltip(options.ToggleLabel).Expanded(options.Expanded).
			Cursor(actionCursor(options.OnToggle == nil)).Disabled(options.OnToggle == nil)
		paint(toggle, button).Children(func() {
			ui.Row(c).Gap(button.ColumnGap).AlignItems(ui.Center).Children(func() {
				text(c, marker, button)
				text(c, options.Label, button)
			})
		})
		if toggle.Clicked() && options.OnToggle != nil {
			options.OnToggle()
		}
		if !options.Expanded {
			return
		}
		body := k.style("historyOCRContent")
		duration := time.Duration(k.palette().TransitionMs) * time.Millisecond
		if options.Message != "" {
			if options.Failed {
				body.Text = rgba(c.Theme().Danger)
			}
			text(c, options.Message, body).Padding(body.Padding[:]...).Wrap()
			return
		}
		result := ui.Scroll(c.Key("history-ocr-text")).FillWidth().MaxHeight(body.MaxHeight).Margin(body.MarginTop, 0, 0)
		if duration > 0 {
			result.Transition(ui.ElementTransition{Size: true, Duration: duration, Enter: &ui.Motion{Y: -3}})
		}
		paint(result, body).Children(func() {
			text(c, options.Text, body).Wrap().Selectable()
		})
	})
}
