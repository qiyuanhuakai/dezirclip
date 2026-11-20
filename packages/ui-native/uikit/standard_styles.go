package uikit

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

func rgba(color ui.Color) [4]uint8 {
	return [4]uint8{color.R, color.G, color.B, color.A}
}

// DefaultWidgetStyle returns the native geometry and paint for a palette.
func DefaultWidgetStyle(widget string, palette theme.Tokens) theme.WidgetStyle {
	panel := rgba(ui.Hex(palette.Panel))
	border := rgba(ui.Hex(palette.Border))
	accent := rgba(ui.Hex(palette.Accent))
	selected := rgba(ui.Hex(palette.Selected))
	muted := rgba(ui.Hex(palette.Muted))
	style := theme.WidgetStyle{Text: rgba(ui.Hex(palette.Text)), Size: float32(palette.FontSize),
		Weight: palette.RegularWeight, Opacity: 1}
	if len(palette.FontFamilies) != 0 {
		style.Font = strings.Join(palette.FontFamilies, ", ")
	}
	surface := func() {
		style.Fill, style.Border = panel, border
		style.BorderWidth, style.Radius = 1, float32(palette.Radius)
	}
	switch widget {
	case "settingsToggleRow":
		style.Height, style.Padding = 34, [4]float32{6, 0, 6, 0}
	case "settingsRangeTrack":
		surface()
		style.Height, style.Radius = 8, 4
	case "settingsRangeFill":
		style.Fill = accent
	case "settingsRangeThumb":
		style.Width, style.Height, style.Radius = 16, 16, 8
		style.Fill, style.Border, style.BorderWidth = accent, border, 1
	case "settingsSwitch":
		style.Width, style.Height = 44, 22
	case "settingsSwitchTrack", "settingsSwitchTrackOn":
		style.Radius, style.Fill = 11, rgba(ui.Hex(palette.Muted).Alpha(.34))
		if widget == "settingsSwitchTrackOn" {
			style.Fill = accent
		}
	case "settingsSwitchThumb", "settingsSwitchThumbOn":
		style.Width, style.Height, style.Radius = 16, 16, 8
		style.Top, style.Left, style.Fill = 3, 3, [4]uint8{255, 255, 255, 255}
		if widget == "settingsSwitchThumbOn" {
			style.Left = 25
		}
	case "hotkeyRow":
		style.Padding = [4]float32{6, 0, 6, 0}
		style.Margin[3] = 18
	case "hotkeyCap", "hotkeyCapRecording":
		surface()
		style.Height, style.Radius, style.Size, style.Weight = 22, 4, 11, 600
		style.Padding, style.Uppercase = [4]float32{0, 6, 0, 6}, true
		if widget == "hotkeyCapRecording" {
			style.Border = accent
		}
	case "hotkeyGroup", "emojiTabs", "emojiGrid", "emojiGroup":
		style.ColumnGap, style.RowGap = 6, 6
	case "emojiContent":
		style.Padding = [4]float32{4, 4, 4, 4}
		style.RowGap = 12
	case "emojiTab", "emojiTabActive":
		surface()
		style.Height, style.Size, style.Weight, style.LetterSpacing, style.Uppercase = 30, 11, 800, .8, true
		if widget == "emojiTabActive" {
			style.Fill, style.Border, style.Text = accent, accent, [4]uint8{255, 255, 255, 255}
		}
	case "emojiGroupTitle":
		style.Size, style.Weight, style.LetterSpacing, style.Uppercase = 10, 800, 1, true
		style.Border, style.BorderWidths = border, &[4]float32{0, 0, 2, 0}
		style.Padding[2] = 4
	case "emojiTile":
		style.Height, style.Size, style.Radius = 32, 18, float32(palette.Radius)
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
		style.ColumnGap = 8
		if widget == "selectSelected" {
			style.Fill = selected
		}
		if widget == "selectOptionDisabled" || widget == "historyEmpty" {
			style.Text = muted
		}
	case "header":
		style.Padding = [4]float32{8, 10, 8, 10}
		style.RowGap = 8
	case "headerTop":
		style.Height = 36
	case "scrollToTop", "scrollToTopHover":
		surface()
		style.Width, style.Height, style.Radius = 32, 32, 16
		if widget == "scrollToTopHover" {
			style.Fill, style.Border = selected, accent
		}
	case "headerSearch":
		style.MarginTop = 4
	case "headerFilters":
		style.ColumnGap, style.Padding = 6, [4]float32{8, 0, 4, 0}
	case "headerTitle", "confirmTitle", "tagTitle":
		style.Size, style.Weight = 18, palette.EmphasisWeight
	case "headerInput", "characterInput", "searchInput":
		surface()
		style.Height, style.Padding = 38, [4]float32{0, 32, 0, 32}
	case "headerInputPlaceholder", "characterInputPlaceholder":
		style.Text = muted
	case "headerInputIcon", "characterInputIcon":
		style.Width, style.Height, style.Left = 14, 14, 12
	case "searchModes":
		surface()
		style.Width, style.Height, style.Padding = 190, 38, [4]float32{2, 2, 2, 2}
	case "searchIcon", "searchClear", "help", "helpActive", "progressClose", "tagClose", "historyAction", "headerButton", "headerButtonActive":
		style.Width, style.Height = 28, 28
		if widget == "searchIcon" {
			style.Width, style.Height, style.Text = 14, 14, muted
		}
		if widget == "headerButtonActive" || widget == "helpActive" {
			style.Fill, style.Radius = selected, float32(palette.Radius)
		}
	case "searchSpinner":
		style.Width, style.Height, style.Radius = 18, 18, 9
		style.Border, style.BorderWidth = border, 2
	case "filter", "filterSelected", "filterAction", "headerFilter", "headerFilterSelected", "searchMode", "searchModeSelected",
		"confirmPrimary", "confirmCancel", "settingsRailItem", "settingsRailActive", "settingsFooterCard":
		surface()
		style.Height, style.Padding = 32, [4]float32{0, 10, 0, 10}
		if widget == "filterSelected" || widget == "headerFilterSelected" || widget == "searchModeSelected" || widget == "settingsRailActive" {
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
		if widget == "historySource" {
			style.ColumnGap = 4
		}
	case "historyActions":
		style.ColumnGap = 4
	case "historyIcon":
		style.Width, style.Height, style.Text = 14, 14, muted
	case "searchModeIcon":
		style.Size, style.Weight = 13, palette.EmphasisWeight
	case "searchModeLabel":
		style.Size, style.Opacity = 10, 0.8
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
	case "tagHeading":
		style.ColumnGap = 6
	case "tagField":
		style.RowGap, style.MarginBottom = 7, 16
	case "tagFieldLabel":
		style.Size = 12
	case "tagInput":
		surface()
		style.Height, style.Padding = 36, [4]float32{0, 8, 0, 8}
	case "tagCloseIcon":
		style.Width, style.Height = 14, 14
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
