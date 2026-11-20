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
	field := ui.TextAreaBase(c, value).Label(label).Placeholder(placeholder)
	return k.styleField(c, field.FillWidth(), "privacySourceExclusion").MinHeight(88)
}
