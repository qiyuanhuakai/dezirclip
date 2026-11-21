package main

import (
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

func (g *gallery) captureActionsFixture(c *ui.Context, kit uikit.Kit) {
	width := float32(500)
	if g.state == "narrow" {
		width = 240
	}
	ui.Column(c).Width(width).Children(func() {
		kit.CaptureSettingsHeading(c, "Capture settings")
		kit.CaptureSettingsActions(c, uikit.CaptureSettingsActionsOptions{
			Label: "Capture now", Region: "Capture region", Full: "Capture full screen", Disabled: g.state == "disabled",
		})
	})
}

func (g *gallery) captureEngineFixture(c *ui.Context, kit uikit.Kit) {
	options := uikit.CaptureEngineStatusOptions{Label: "OCR engine status", Retry: "Retry"}
	switch g.state {
	case "available":
		options.Status, options.Available = "Tesseract (Available)", true
	case "unavailable":
		options.Status = "Tesseract (Not available)"
	case "failed":
		options.Status = "Could not check the OCR engine"
	case "loading":
		options.Status, options.Retry, options.Disabled = "Loading…", "Processing…", true
	}
	ui.Column(c).Width(430).Children(func() { kit.CaptureEngineStatus(c, options) })
}
