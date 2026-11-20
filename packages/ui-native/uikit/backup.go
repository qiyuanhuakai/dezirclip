package uikit

import "github.com/egoist/mygo/ui"

func (k Kit) BackupDialog(c *ui.Context, open *bool, title string, busy bool, body, footer func()) {
	style := k.style("backupDialog")
	ui.DialogBase(c, open, func(backdrop, panel ui.Element) {
		backdrop.Background(styleColor(k.style("backupBackdrop").Fill))
		width, height := c.Size()
		paint(panel.Label(title).Width(min(style.Width, max(0, width-40))), style)
		if busy && panel.Dismissed() {
			*open = true
		}
		maxHeight := max(40, height-40-style.Padding[0]-style.Padding[2]-style.BorderWidth*2)
		ui.Column(c).FillWidth().MaxHeight(maxHeight).Children(func() {
			titleStyle := k.style("backupTitle")
			paint(ui.Row(c).AlignSelf(ui.Start).Shrink(0).Margin(0, 0, titleStyle.MarginBottom), titleStyle).
				Children(func() { text(c, title, titleStyle).Wrap() })
			ui.Scroll(c).FillWidth().MinHeight(0).Shrink(1).Children(func() {
				ui.Column(c).FillWidth().Children(body)
			})
			footer()
		})
	})
}

func (k Kit) backupField(c *ui.Context, label string, content func()) {
	style, labelStyle := k.style("backupField"), k.style("backupLabel")
	ui.Column(c).FillWidth().Gap(labelStyle.MarginBottom).Margin(0, 0, style.MarginBottom).Children(func() {
		text(c, label, labelStyle).Wrap()
		content()
	})
}

func (k Kit) BackupChoices(c *ui.Context, label string, selected *string, options []SelectOption, disabled bool) bool {
	style := k.style("backupChoices")
	changed := false
	k.backupField(c, label, func() {
		ui.RadioGroup(c, func() {
			for _, option := range options {
				radio := ui.RadioBase(c, selected, option.Value).Label(option.Label).
					Disabled(disabled || option.Disabled).FocusRing(true).AlignItems(ui.Center)
				widget := "backupChoice"
				if radio.Hovered() {
					widget += "Hover"
				}
				choice := k.style(widget)
				paint(radio.Gap(choice.ColumnGap), k.animatePaint(radio, choice)).Children(func() {
					widget := "backupRadio"
					if *selected == option.Value {
						widget += "Checked"
					}
					mark := k.style(widget)
					box := ui.Box(c).Size(mark.Width, mark.Height).Shrink(0).PassThrough()
					paint(box, k.animatePaint(box, mark)).Radius(mark.Width / 2)
					text(c, option.Label, choice).Wrap().PassThrough()
				})
				changed = radio.Changed() || changed
			}
		}).Label(label).Row().Wrap().Gap(style.ColumnGap)
	})
	return changed
}

type BackupFieldOptions struct {
	Label, Placeholder, Error, BrowseLabel string
	Value                                  *string
	Password, ReadOnly, Disabled           bool
	OnBrowse                               func()
}

func (k Kit) BackupField(c *ui.Context, options BackupFieldOptions) {
	k.backupField(c, options.Label, func() {
		field := func() ui.Element {
			edit := ui.TextInputBase(c, options.Value).Label(options.Label).Placeholder(options.Placeholder).
				ReadOnly(options.ReadOnly || options.Disabled).Disabled(options.Disabled)
			if options.Password {
				edit.Password()
			}
			return k.styleField(c, edit, "backupInput").MinWidth(0)
		}
		if options.BrowseLabel == "" {
			field().FillWidth()
		} else {
			ui.Row(c).FillWidth().AlignItems(ui.Center).Gap(k.style("backupPathRow").ColumnGap).Children(func() {
				input := field().Grow(1)
				if *options.Value != "" {
					input.Tooltip(*options.Value)
				}
				if k.backupButton(c, "backupBrowse", options.BrowseLabel, options.Disabled).Shrink(0).Clicked() && options.OnBrowse != nil {
					options.OnBrowse()
				}
			})
		}
		if options.Error != "" {
			k.BackupError(c, options.Error)
		}
	})
}

func (k Kit) BackupError(c *ui.Context, message string) {
	text(c, message, k.style("backupError")).Wrap()
}

func (k Kit) BackupPreview(c *ui.Context, label string, entries []string) {
	if len(entries) == 0 {
		return
	}
	k.backupField(c, label, func() {
		style := k.style("backupPreview")
		paint(ui.Column(c).FillWidth(), style).Children(func() {
			ui.Scroll(c).FillWidth().MaxHeight(style.Height).Children(func() {
				ui.Column(c).FillWidth().Children(func() {
					for i, entry := range entries {
						row := k.style("backupPreviewRow")
						if i == len(entries)-1 {
							row.BorderWidth, row.BorderWidths = 0, nil
						}
						paint(text(c, entry, row).FillWidth().SingleLine().Tooltip(entry), row)
					}
				})
			})
		})
	})
}

func (k Kit) BackupActions(c *ui.Context, cancel, submit string, busy, disabled bool, onCancel, onSubmit func()) {
	style := k.style("backupActions")
	ui.Row(c).FillWidth().Shrink(0).Margin(style.MarginTop, 0, 0).Gap(style.ColumnGap).Wrap().Justify(ui.End).Children(func() {
		if k.backupButton(c, "backupCancel", cancel, busy).Clicked() && onCancel != nil {
			onCancel()
		}
		if k.backupButton(c, "backupSubmit", submit, busy || disabled).Clicked() && onSubmit != nil {
			onSubmit()
		}
	})
}

func (k Kit) backupButton(c *ui.Context, widget, label string, disabled bool) ui.Element {
	button := ui.ButtonBase(c).Label(label).Disabled(disabled).AlignItems(ui.Center).FocusRing(true)
	style := k.style(widget)
	if disabled {
		style.Opacity = 0.5
	} else if button.Hovered() && widget != "backupBrowse" {
		style = k.style(widget + "Hover")
	}
	style = k.animatePaint(button, style)
	paint(button.Height(style.Height), style).Children(func() {
		text(c, label, style).Wrap().PassThrough()
	})
	return button
}
