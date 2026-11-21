package uikit

import "github.com/egoist/mygo/ui"

var captureCamera = icon("Camera")

// CaptureSettingsHeading identifies the capture settings in both pane layouts.
func (k Kit) CaptureSettingsHeading(c *ui.Context, label string) {
	k.SettingsField(c, SettingsFieldOptions{}).MinHeight(34).ColumnSpan(-1).Children(func() {
		k.SettingsLabel(c, label).Wrap()
	})
}

type CaptureSettingsActionsOptions struct {
	Label, Region, Full string
	Error               string
	Disabled            bool
	OnRegion, OnFull    func()
}

func (k Kit) CaptureSettingsActions(c *ui.Context, options CaptureSettingsActionsOptions) {
	k.SettingsField(c, SettingsFieldOptions{Detail: true}).ColumnSpan(-1).Children(func() {
		k.captureSettingsLabel(c, options.Label)
		ui.Row(c).AlignItems(ui.Center).Wrap().Gap(8).MinWidth(0).Children(func() {
			if k.settingsAction(c, options.Region, options.Disabled, 0, nil, captureCamera).Clicked() && options.OnRegion != nil {
				options.OnRegion()
			}
			if k.settingsAction(c, options.Full, options.Disabled, 0, nil, captureCamera).Clicked() && options.OnFull != nil {
				options.OnFull()
			}
		})
	})
	if options.Error != "" {
		ui.Text(c, options.Error).ColumnSpan(-1).TextColor(c.Theme().Danger).Wrap()
	}
}

type CaptureEngineStatusOptions struct {
	Label, Status, Retry string
	Available, Disabled  bool
	OnRetry              func()
}

func (k Kit) CaptureEngineStatus(c *ui.Context, options CaptureEngineStatusOptions) {
	k.SettingsField(c, SettingsFieldOptions{}).ColumnSpan(-1).Children(func() {
		k.captureSettingsLabel(c, options.Label)
		ui.Row(c).AlignItems(ui.Center).Wrap().Justify(ui.End).Gap(8).MinWidth(0).Children(func() {
			k.SettingsStatusBadge(c, options.Status, options.Available)
			if k.settingsAction(c, options.Retry, options.Disabled, 28, &[4]float32{5, 10, 5, 10}, nil).Clicked() && options.OnRetry != nil {
				options.OnRetry()
			}
		})
	})
}

func (k Kit) SettingsStatusBadge(c *ui.Context, label string, available bool) {
	color := c.Theme().TextMuted
	if available {
		color = ui.Hex("#26724a")
		if k.Mode == "dark" {
			color = ui.Hex("#70d69b")
		}
	}
	ui.Row(c).AlignItems(ui.Center).Padding(4, 10).Gap(6).Radius(999).
		Border(1, color.Alpha(.28)).Background(color.Alpha(.14)).Role(ui.RoleStatus).Label(label).Children(func() {
		ui.Box(c).Size(6, 6).Radius(3).Background(color).Shrink(0).PassThrough()
		ui.Text(c, label).FontSize(12).FontWeight(600).FixedLineHeight(16.8).TextColor(color).Wrap()
	})
}

func (k Kit) FixedCaptureRenderer(c *ui.Context, label, value string) {
	k.SettingsField(c, SettingsFieldOptions{Detail: true}).Children(func() {
		k.captureSettingsLabel(c, label)
		ui.Text(c, value).FontSize(12).TextColor(c.Theme().TextMuted).Wrap()
	})
}

func (k Kit) captureSettingsLabel(c *ui.Context, label string) {
	// The horizontal settings label reserves the same 9 em as its Web counterpart.
	k.SettingsLabel(c, label).Width(144).MaxWidthPercent(100).Grow(1).Shrink(0).Wrap()
}
