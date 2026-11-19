package uikit

import (
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

func rgba(color ui.Color) [4]uint8 {
	return [4]uint8{color.R, color.G, color.B, color.A}
}

// These defaults are native control dimensions and use the supplied semantic
// palette. Complete Web lookalike catalogs can be passed separately to NewKit.
func standardWidgetStyle(widget string, palette theme.Tokens) theme.WidgetStyle {
	panel := rgba(ui.Hex(palette.Panel))
	border := rgba(ui.Hex(palette.Border))
	accent := rgba(ui.Hex(palette.Accent))
	selected := rgba(ui.Hex(palette.Selected))
	muted := rgba(ui.Hex(palette.Muted))
	style := theme.WidgetStyle{Text: rgba(ui.Hex(palette.Text)), Size: float32(palette.FontSize),
		Weight: palette.RegularWeight, Opacity: 1}
	if len(palette.FontFamilies) != 0 {
		style.Font = palette.FontFamilies[0]
	}
	surface := func() {
		style.Fill, style.Border = panel, border
		style.BorderWidth, style.Radius = 1, float32(palette.Radius)
	}
	switch widget {
	case "select", "selectHover", "selectFocus", "selectDisabled":
		surface()
		style.Width, style.Height = 224, 36
		switch widget {
		case "selectHover":
			style.Fill = selected
		case "selectFocus":
			style.Border = accent
		case "selectDisabled":
			style.Text = muted
		}
	case "selectIndicator", "disclosureChevron":
		style.Width, style.Height, style.Text = 26, 26, muted
	case "selectMenu", "history", "confirm", "tag", "toast", "progress", "progressDone", "progressError":
		surface()
		style.Width, style.Height, style.Padding = 304, 44, [4]float32{10, 12, 10, 12}
		switch widget {
		case "selectMenu":
			style.Width, style.MarginTop, style.Padding = 224, 4, [4]float32{}
		case "confirm", "tag":
			style.Width, style.Height, style.Padding = 360, 100, [4]float32{16, 16, 16, 16}
		case "progress", "progressDone", "progressError":
			style.Width, style.Height = 252, 54
		}
	case "selectMenuList":
		style.Padding = [4]float32{4, 4, 4, 4}
	case "selectOption", "selectSelected", "selectOptionDisabled", "historyItem", "historyEmpty":
		style.Height, style.Padding = 32, [4]float32{0, 8, 0, 8}
		if widget == "selectSelected" {
			style.Fill = selected
		}
		if widget == "selectOptionDisabled" || widget == "historyEmpty" {
			style.Text = muted
		}
	case "header":
		style.Padding = [4]float32{8, 10, 8, 10}
	case "headerTop":
		style.Height = 36
	case "headerSearch":
		style.Padding = [4]float32{8, 0, 0, 0}
	case "headerTitle", "confirmTitle", "tagTitle":
		style.Size, style.Weight = 18, palette.EmphasisWeight
	case "headerInput", "searchInput":
		surface()
		style.Height, style.Padding = 38, [4]float32{0, 32, 0, 32}
	case "searchModes":
		surface()
		style.Width, style.Height, style.Padding = 190, 38, [4]float32{2, 2, 2, 2}
	case "searchIcon", "searchClear", "help", "helpActive", "progressClose", "tagClose", "historyAction", "headerButton", "headerButtonActive":
		style.Width, style.Height = 28, 28
		if widget == "headerButtonActive" || widget == "helpActive" {
			style.Fill, style.Radius = selected, float32(palette.Radius)
		}
	case "searchSpinner":
		style.Width, style.Height, style.Radius = 18, 18, 9
		style.Border, style.BorderWidth = border, 2
	case "filter", "filterSelected", "filterAction", "headerFilter", "searchMode", "searchModeSelected",
		"confirmPrimary", "confirmCancel", "settingsRailItem", "settingsRailActive", "settingsFooterCard":
		surface()
		style.Height, style.Padding = 32, [4]float32{0, 10, 0, 10}
		if widget == "filterSelected" || widget == "searchModeSelected" || widget == "settingsRailActive" {
			style.Fill, style.Border = selected, accent
		}
		if widget == "confirmPrimary" {
			style.Fill, style.Border = accent, accent
			style.Text = rgba(ui.LightTheme().AccentText)
		}
	case "settingsRail":
		style.Width = 156
	case "disclosure":
		style.Height, style.Weight = 34, palette.EmphasisWeight
	case "settingsFooterLink":
		style.Text, style.Height, style.Size = accent, 24, 12
	case "helpText", "searchCount", "historySource", "historyTime", "settingsFooterVersion", "progressLabel":
		style.Text, style.Size = muted, 12
	case "progressStatus", "searchError":
		style.Text = accent
	case "progressTrack":
		style.Fill, style.Height, style.Radius = border, 4, 2
	case "progressFill", "mark":
		style.Fill, style.Radius = selected, 2
		if widget == "progressFill" {
			style.Fill = accent
		}
	case "confirmBackdrop", "tagBackdrop":
		style.Fill = [4]uint8{0, 0, 0, 120}
	case "historyCard", "historyCardSelected", "historyCardCompact":
		surface()
		style.Height, style.Padding = 56, [4]float32{8, 10, 8, 10}
		style.Margin = [4]float32{0, 6, 2, 6}
		if widget == "historyCardSelected" {
			style.Fill, style.Border = selected, accent
		} else if widget == "historyCardCompact" {
			style.Height, style.Padding = 44, [4]float32{4, 8, 4, 8}
		}
	case "historyMeta":
		style.Height = 22
	case "attachmentName":
		style.Height = 19
	case "attachmentIcon":
		style.Text = muted
	case "attachmentImage":
		style.Border, style.BorderWidth, style.Radius = border, 1, 4
	case "previewToolbar":
		style.Padding, style.Border = [4]float32{4, 8, 4, 8}, border
		style.BorderWidths = &[4]float32{0, 0, 1, 0}
	case "previewToolbarTitle":
		style.Size = 12
	case "previewToolbarButton", "previewToolbarActive", "previewToolbarActual":
		surface()
		style.Width, style.Height = 26, 26
		if widget == "previewToolbarActive" {
			style.Fill = selected
		} else if widget == "previewToolbarActual" {
			style.Width, style.Size = 40, 10
		}
	}
	return style
}
