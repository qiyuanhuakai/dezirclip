package uikit

import "github.com/egoist/mygo/ui"

// TagPicker provides keyboard and text filtering in a compact toolbar.
func (k Kit) TagPicker(c *ui.Context, value *string, label, placeholder string, choices []string, disabled bool) (ui.Element, bool) {
	return k.combo(c, value, label, placeholder, choices, disabled, 27)
}

func (k Kit) TagToolbar(c *ui.Context, leading func(), actions []HeaderAction, trailing func()) {
	row := k.style("tagToolbar")
	paint(ui.Row(c).FillWidth().MinHeight(row.Height).Gap(row.ColumnGap).AlignItems(ui.Center), row).Children(func() {
		leading()
		for _, action := range actions {
			button := k.TagToolbarButton(c, action)
			if action.Run != nil && button.Clicked() {
				action.Run()
			}
		}
		if trailing != nil {
			trailing()
		}
	})
}

func (k Kit) TagToolbarButton(c *ui.Context, action HeaderAction) ui.Element {
	style := k.style("tagToolbarButton")
	if action.Active {
		style = k.style("tagToolbarButtonActive")
	}
	button := ui.ButtonBase(c).Label(action.Label).Tooltip(action.Label).Size(27, 27).
		Selected(action.Active).Center().Shrink(0).Disabled(action.Disabled)
	paint(button, k.interactive(c, button, style, action.Active)).Children(func() {
		if symbol := headerIcons[action.Icon]; symbol != nil {
			ui.Icon(c, symbol).Size(16, 16).TextColor(styleColor(style.Text)).PassThrough()
		}
	})
	return button
}
