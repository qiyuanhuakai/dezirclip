package uikit

import "github.com/egoist/mygo/ui"

type UpdateAction uint8

const (
	UpdateCheck UpdateAction = iota
	UpdateInstall
	UpdateReleases
)

var updateActionIcons = map[UpdateAction]*ui.SVG{
	UpdateCheck: icon("RefreshCw"), UpdateInstall: settingsExportIcon, UpdateReleases: headerIcons["open"],
}

func (k Kit) UpdateActions(c *ui.Context, draw func()) {
	style := k.style("updateActions")
	ui.Row(c).FillWidth().Wrap().AlignItems(ui.Center).GapX(style.ColumnGap).GapY(style.RowGap).
		Margin(style.MarginTop, 0, style.MarginBottom).Children(draw)
}

func (k Kit) UpdateAction(c *ui.Context, action UpdateAction, label string, disabled bool) ui.Element {
	button := ui.ButtonBase(c).Label(label).Disabled(disabled).FocusRing(true).AlignItems(ui.Center)
	widget := "updateAction"
	switch {
	case disabled:
		widget = "updateActionDisabled"
	case button.Hovered():
		widget = "updateActionHover"
	}
	style := k.style(widget)
	paint(button.Height(style.Height).Gap(style.ColumnGap), k.animatePaint(button, style)).Children(func() {
		if symbol := updateActionIcons[action]; symbol != nil {
			ui.Icon(c, symbol).Size(14, 14).TextColor(styleColor(style.Text)).Shrink(0).PassThrough()
		}
		text(c, label, style).SingleLine().PassThrough()
	})
	return button
}

// UpdateProgressOptions describes a download or an update being installed.
// A negative fraction represents a download of unknown size.
type UpdateProgressOptions struct {
	Status, Amount, Label, Verified string
	Fraction                        float64
}

func (k Kit) UpdateVersionRow(c *ui.Context, label, version string) {
	style := k.style("updateVersion")
	if version == "" {
		version = "—"
	} else {
		version = "v" + version
	}
	ui.Row(c).FillWidth().Gap(style.ColumnGap).Margin(style.MarginTop, 0, style.MarginBottom).
		Wrap().Justify(ui.SpaceBetween).Children(func() {
		text(c, label, style).Wrap()
		text(c, version, k.style("updateVersionValue")).Wrap().Selectable()
	})
}

func (k Kit) UpdateProgress(c *ui.Context, options UpdateProgressOptions) {
	style, track := k.style("updateProgressText"), k.style("updateProgressTrack")
	ui.Column(c).FillWidth().Role(ui.RoleStatus).Children(func() {
		ui.Row(c).FillWidth().Gap(8).Wrap().Justify(ui.SpaceBetween).Children(func() {
			text(c, options.Status, style).Wrap()
			text(c, options.Amount, style).SingleLine()
		})
		original := c.Theme()
		palette := *original
		palette.Accent = styleColor(k.style("updateProgressFill").Fill)
		c.SetTheme(&palette)
		ui.Progress(c, options.Fraction).FillWidth().Height(track.Height).Margin(track.MarginTop, 0, 0).
			Background(styleColor(track.Fill)).Radius(track.Height / 2).Label(options.Label).Value(options.Amount)
		c.SetTheme(original)
		if options.Verified != "" {
			label := k.style("updateProgressVerified")
			ui.Row(c).FillWidth().Gap(5).Margin(label.MarginTop, 0, label.MarginBottom).AlignItems(ui.Center).Children(func() {
				ui.Icon(c, updateVerifiedIcon).Size(14, 14).TextColor(styleColor(label.Text)).Shrink(0)
				text(c, options.Verified, label).Wrap()
			})
		}
	})
}

var updateVerifiedIcon = icon("ShieldCheck")
