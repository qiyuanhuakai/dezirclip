package main

import (
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

func (g *gallery) cliSettingsFixture(c *ui.Context, kit uikit.Kit) {
	const cli = `C:\Program Files\DezirClip`
	options := uikit.CLISettingsOptions{
		Title: "Command-line tools", Description: "Run dzc to browse clipboard history.", HelpLabel: "Setting help",
		Fields:      []uikit.CLISettingField{{Label: "Installed CLI", Value: cli + `\dzc.exe`, Path: true}, {Label: "Version", Value: "0.2.0-beta.5"}},
		Status:      uikit.CLISettingsStatus{Label: "PATH status", Value: "dzc (not on PATH)", Action: "Add to PATH"},
		GuidesTitle: "Agent guides", GuidesDescription: "Open a packaged guide to import it.", GuideOpenLabel: "Open guide",
		Guides: []uikit.CLISettingGuide{
			{Label: "CLI guide", Path: cli + `\skills\dzc-cli`, Fallback: "dzc-cli"},
			{Label: "Theme guide", Path: cli + `\skills\dezirclip-theme`, Fallback: "dezirclip-theme"},
		},
	}
	width := float32(500)
	switch g.state {
	case "narrow":
		width = 270
	case "registered":
		options.Status.Value, options.Status.Action = "dzc (configured for this user)", ""
	case "conflict":
		options.Status.Value, options.Status.Action = "dzc (another PATH copy)", ""
		options.Conflict = []uikit.CLISettingField{{Label: "Terminal CLI path", Value: `C:\Old tools\dzc.exe`, Path: true}, {Label: "Terminal CLI version", Value: "0.1.0"}}
	case "failed":
		options.Status.Value, options.Status.Action = "Could not read CLI status", "Retry"
		options.Guides[1].Path = ""
	}
	ui.Column(c).Width(width).Children(func() { kit.CLISettings(c, options) })
}
