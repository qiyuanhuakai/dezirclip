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
	case "settingsDataBlock":
		style.RowGap, style.Padding = 6, [4]float32{6, 0, 8, 0}
	case "settingsRemove", "settingsRemoveIcon":
		surface()
		style.Text = rgba(ui.LightTheme().Danger)
		style.Width, style.Height, style.Padding = 28, 30, [4]float32{4, 8, 4, 8}
		if widget == "settingsRemoveIcon" {
			style.Height, style.Padding = 28, [4]float32{}
		}
	case "settingsDataHeader", "settingsDataActions":
		style.ColumnGap = 8
	case "settingsDataLabel":
		style.Text, style.Weight = rgba(ui.Hex(palette.Text)), palette.EmphasisWeight
	case "hotkeyRemove":
		surface()
		style.Width, style.Height = 24, 24
	case "tagToolbar":
		style.Height, style.ColumnGap = 30, 2
		style.Padding = [4]float32{0, 7, 0, 3}
	case "tagToolbarButton", "tagToolbarButtonActive":
		style = DefaultWidgetStyle("headerButton", palette)
		style.Padding = [4]float32{}
		if widget == "tagToolbarButtonActive" {
			style.Fill, style.Border = selected, accent
		}
	case "settingsDataPath", "settingsDataAction":
		surface()
		style.Height, style.Padding, style.Size = 30, [4]float32{5, 9, 5, 9}, 12
		if widget == "settingsDataPath" {
			style.Font, style.Weight = "monospace", 700
		}
	case "emojiAddTile", "emojiAddTileHover", "emojiFavoriteTile":
		surface()
		style.Radius, style.Size = 8, 11
		style.Padding = [4]float32{3, 3, 3, 3}
		if widget != "emojiFavoriteTile" {
			style.Dashed, style.BorderWidth, style.Weight = true, 2, 800
		}
		if widget == "emojiAddTileHover" {
			style.Border, style.Text = accent, accent
		}
	case "emojiFavoriteRemove":
		style.Fill, style.Text, style.Radius = [4]uint8{0, 0, 0, 217}, [4]uint8{255, 255, 255, 255}, 6
	case "settingsGroup", "settingsGroupOpen":
		surface()
	case "settingsGroupHeader", "settingsGroupHeaderOpen":
		style.Height, style.Size, style.Weight = 42, 14, palette.EmphasisWeight
		style.Padding = [4]float32{8, 12, 8, 12}
	case "settingsGroupContent":
		style.Padding = [4]float32{8, 12, 12, 12}
	case "settingsGroupChevron":
		style.Text = muted
	case "settingsSegments":
		surface()
		style.Padding, style.ColumnGap = [4]float32{2, 2, 2, 2}, 2
	case "settingsChoiceCards":
		style.ColumnGap = 8
	case "settingsSegment", "settingsSegmentHover", "settingsSegmentActive", "settingsChoiceCard", "settingsChoiceCardHover", "settingsChoiceCardActive":
		surface()
		style.Height, style.Size, style.Weight = 36, 12, palette.EmphasisWeight
		style.Padding = [4]float32{0, 12, 0, 12}
		if strings.HasPrefix(widget, "settingsSegment") {
			style.Height, style.BorderWidth, style.Fill = 30, 0, [4]uint8{}
		}
		if strings.HasSuffix(widget, "Active") || strings.HasSuffix(widget, "Hover") {
			style.Fill = selected
		}
	case "textField", "textFieldFocus", "numberField", "numberFieldFocus":
		surface()
		style.Height, style.Padding = 36, [4]float32{6, 10, 6, 10}
		if strings.HasPrefix(widget, "number") {
			style.Height, style.Padding, style.Size = 28, [4]float32{4, 8, 4, 8}, 12
		}
		if strings.HasSuffix(widget, "Focus") {
			style.Border = accent
		}
	case "contextMenu", "contextSubmenu":
		surface()
		style.Width, style.Padding = 180, [4]float32{4, 0, 4, 0}
		if widget == "contextSubmenu" {
			style.Width = 168
		}
	case "contextMenuItem", "contextMenuItemActive", "contextSubmenuItem", "contextSubmenuItemActive":
		style.Height, style.Padding, style.Size = 34, [4]float32{0, 12, 0, 12}, 12
		if strings.HasPrefix(widget, "contextSubmenuItem") {
			style.Height = 30
		}
		if strings.HasSuffix(widget, "Active") {
			style.Fill = selected
		}
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
	case "hotkeyEditor":
		style.Padding, style.RowGap = [4]float32{6, 0, 6, 0}, 6
	case "hotkeyEditorHeader":
		style.Height, style.ColumnGap, style.RowGap = 34, 12, 8
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
	case "historyActions", "historyActionsCompact":
		style.ColumnGap = 4
		if widget == "historyActionsCompact" {
			style.TranslateY = 5
		}
	case "historyActionsCompactAnchor":
		style.Top, style.Right = -8, 8
	case "historyActionCompact", "historyActionCompactHover", "historyActionCompactActive", "historyActionCompactActiveHover":
		surface()
		style.Width, style.Height, style.Padding = 20, 20, [4]float32{3, 3, 3, 3}
		if widget != "historyActionCompact" {
			style.Fill = selected
		}
	case "historyActionIcon":
		style.Width, style.Height = 12, 12
	case "historyActionCompactIcon":
		style.Width, style.Height = 14, 14
	case "historyContentCompact":
		style = DefaultWidgetStyle("historyContent", palette)
		style.LineHeight = 1.3
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
