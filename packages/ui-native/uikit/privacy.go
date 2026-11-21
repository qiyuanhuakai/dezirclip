package uikit

import (
	"strconv"

	"github.com/egoist/mygo/ui"
)

type PrivacyRuleCardOptions struct {
	Title, Source, Scope, Identifier, Created      string
	RemoveLabel, CancelLabel, ConfirmLabel, Prompt string
	Confirming, Disabled                           bool
	OnRequestRemove, OnCancel, OnConfirm           func()
}

func (k Kit) PrivacyRulesHeading(c *ui.Context, label string, count *int) {
	style := k.style("privacyRulesHeading")
	paint(ui.Row(c).FillWidth().AlignItems(ui.Center).Gap(8), style).Children(func() {
		text(c, label, k.style("privacyRulesHeadingLabel")).Grow(1).Wrap()
		if count != nil {
			text(c, strconv.Itoa(*count), k.style("privacyRuleSource")).SingleLine()
		}
	})
}

func (k Kit) PrivacyRuleGrid(c *ui.Context, columns int, content func()) {
	ui.Grid(c).FillWidth().Columns(max(1, columns)).GapX(20).GapY(12).AlignItems(ui.Start).
		Margin(8, 0, 0, 0).Children(content)
}

func (k Kit) PrivacyRuleCard(c *ui.Context, options PrivacyRuleCardOptions) {
	style := k.style("privacyRuleCard")
	paint(ui.Column(c).AlignSelf(ui.Stretch).MinWidth(0).Gap(8), style).Children(func() {
		ui.Row(c).FillWidth().Wrap().AlignItems(ui.Center).GapX(10).GapY(4).Children(func() {
			text(c, options.Title, k.style("privacyRuleTitle")).Wrap()
			text(c, options.Source, k.style("privacyRuleSource")).MinWidth(0).Wrap()
		})
		body := k.style("privacyRuleScope")
		text(c, options.Scope, body).Wrap()
		ui.Column(c).FillWidth().Gap(4).Children(func() {
			text(c, options.Identifier, body).Wrap()
			text(c, options.Created, body).Wrap()
		})
		if !options.Confirming {
			if k.privacyRuleAction(c, options.RemoveLabel, "privacyRuleAction", options.Disabled).Clicked() && options.OnRequestRemove != nil {
				options.OnRequestRemove()
			}
			return
		}
		ui.Column(c).FillWidth().Gap(8).Role(ui.RoleGroup).Label(options.ConfirmLabel).Children(func() {
			text(c, options.Prompt, body).Wrap()
			ui.Row(c).Wrap().Gap(6).AlignItems(ui.Center).Children(func() {
				if k.privacyRuleAction(c, options.CancelLabel, "privacyRuleCancel", options.Disabled).Clicked() && options.OnCancel != nil {
					options.OnCancel()
				}
				if k.SettingsRemoveButton(c, options.RemoveLabel, false, options.Disabled).Clicked() && options.OnConfirm != nil {
					options.OnConfirm()
				}
			})
		})
	})
}

func (k Kit) privacyRuleAction(c *ui.Context, label, widget string, disabled bool) ui.Element {
	style := k.style(widget)
	button := ui.ButtonBase(c).Label(label).Height(style.Height).AlignSelf(ui.Start).Center().
		FocusRing(true).Disabled(disabled)
	paint(button, k.interactive(c, button, style, false)).
		Children(func() { text(c, label, style).SingleLine().PassThrough() })
	return button
}

// SourceExclusionField draws the full-width input used for application exclusions.
func (k Kit) SourceExclusionField(c *ui.Context, value *string, label, placeholder string) ui.Element {
	return k.resizablePrivacyTextArea(c, value, label, placeholder, "privacySourceExclusion", 88)
}

type PrivacyDetectionOption struct {
	Value, Label string
	Checked      bool
}

// PrivacyDetectionOptions lays out labeled switches that wrap as a group.
func (k Kit) PrivacyDetectionOptions(c *ui.Context, options []PrivacyDetectionOption, disabled bool, change func(string, bool)) {
	group, optionStyle, labelStyle := k.style("privacyDetectionOptions"), k.style("privacyDetectionOption"), k.style("privacyDetectionLabel")
	switchStyle := k.style("settingsSwitch")
	ui.Column(c).FillWidth().MinWidth(0).Padding(group.Margin[:]...).Children(func() {
		ui.Row(c).FillWidth().MinWidth(0).Wrap().GapX(group.ColumnGap).GapY(group.RowGap).Children(func() {
			for _, option := range options {
				checked := option.Checked
				control := ui.CheckboxBase(c.Key(option.Value), &checked).Label(option.Label).
					AlignItems(ui.Center).Gap(optionStyle.ColumnGap).FocusRing(true).Disabled(disabled)
				control.Children(func() {
					track := ui.Box(c).Size(switchStyle.Width, switchStyle.Height).Shrink(0).PassThrough()
					if disabled {
						track.Opacity(k.style("settingsSwitchDisabled").Opacity)
					}
					k.paintSettingsSwitch(c, track, checked)
					text(c, option.Label, labelStyle).Wrap().PassThrough()
				})
				if control.Changed() && change != nil {
					change(option.Value, checked)
				}
			}
		})
	})
}

// PrivacyPatternsField draws the indented multiline input for custom detection patterns.
func (k Kit) PrivacyPatternsField(c *ui.Context, value *string, label, placeholder string) ui.Element {
	style := k.style("privacyPatternsField")
	var field ui.Element
	padding := k.style("privacyPatternsContainer").Padding
	ui.Column(c).FillWidth().MinWidth(0).Padding(padding[:]...).Children(func() {
		field = k.resizablePrivacyTextArea(c, value, label, placeholder, "privacyPatternsField", style.MinHeight)
	})
	return field
}

func (k Kit) resizablePrivacyTextArea(c *ui.Context, value *string, label, placeholder, widget string, minimum float32) ui.Element {
	style := k.style(widget)
	minimum = max(minimum, style.MinHeight)
	wrapper := ui.Column(c).FillWidth().MinWidth(0)
	type resizeState struct {
		height, startHeight, startPointer float32
		active                            bool
	}
	resize := ui.Local(wrapper, "text-area-resize", func() resizeState {
		return resizeState{height: max(minimum, style.Height)}
	})
	height := max(minimum, resize.height)
	var field ui.Element
	wrapper.Children(func() {
		field = k.styleField(c, ui.TextAreaBase(c, value).Label(label).Placeholder(placeholder).FillWidth(), widget).Height(height)
		ui.Box(c).Absolute().Bottom(1).Right(1).Size(12, 12).Role(ui.RoleNone).Cursor(ui.CursorResizeNS).
			HandleInput(func(event ui.InputEvent) bool {
				if event.Button != 0 {
					return false
				}
				// The corner moves with the field. Measure from the field's top edge.
				pointer := height + event.Y
				switch event.Kind {
				case ui.InputPointerDown:
					resize.active, resize.startHeight, resize.startPointer = true, height, pointer
					return true
				case ui.InputPointerMove:
					if resize.active {
						resize.height = max(minimum, resize.startHeight+pointer-resize.startPointer)
						return true
					}
				case ui.InputPointerUp:
					if resize.active {
						resize.active = false
						return true
					}
				}
				return false
			}).Draw(func(p *ui.Painter, r ui.Rect) {
			x, y := r.X+r.W-2, r.Y+r.H-2
			color := styleColor(style.Text).Alpha(0.55)
			var grip ui.Path
			grip.MoveTo(x-4, y).LineTo(x, y-4)
			grip.MoveTo(x-8, y).LineTo(x, y-8)
			p.StrokePath(&grip, 1, color)
		})
	})
	return field
}
