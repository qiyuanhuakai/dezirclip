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
	case "updateActions":
		style.ColumnGap, style.RowGap, style.MarginTop, style.MarginBottom = 8, 8, 10, 10
	case "updateAction", "updateActionHover", "updateActionDisabled":
		surface()
		style.Height, style.ColumnGap, style.Weight = 30, 6, 600
		style.Padding = [4]float32{4, 12, 4, 12}
		if widget == "updateActionHover" {
			style.Fill, style.Border = selected, accent
		}
		if widget == "updateActionDisabled" {
			style.Opacity = 0.6
		}
	case "updateVersion", "updateVersionValue":
		style.ColumnGap, style.MarginTop, style.MarginBottom = 8, 8, 8
		if widget == "updateVersionValue" {
			style.Weight = palette.EmphasisWeight
		}
	case "updateProgressTrack":
		style.Height, style.MarginTop, style.Fill = 8, 8, rgba(ui.Hex(palette.Muted).Alpha(0.2))
	case "updateProgressFill":
		style.Fill = accent
	case "updateProgressVerified":
		style.Text, style.MarginTop, style.MarginBottom = muted, 10, 10
	case "backupDialog":
		surface()
		style.Width, style.BorderWidth, style.Padding = 500, 4, [4]float32{16, 16, 16, 16}
	case "backupBackdrop":
		style.Fill = [4]uint8{0, 0, 0, 128}
	case "backupTitle":
		style.Fill, style.Text = accent, [4]uint8{255, 255, 255, 255}
		style.Weight, style.Size, style.Uppercase, style.MarginBottom = 900, 14, true, 16
		style.Padding, style.Radius = [4]float32{4, 8, 4, 8}, 4
	case "backupField":
		style.MarginBottom = 12
	case "backupLabel":
		style.Weight, style.Uppercase, style.MarginBottom = 700, true, 4
	case "backupChoices":
		style.ColumnGap = 12
	case "backupChoice", "backupChoiceHover", "backupChoiceSelected", "backupChoiceSelectedHover":
		surface()
		style.Padding, style.ColumnGap = [4]float32{6, 10, 6, 10}, 6
		if strings.HasSuffix(widget, "Hover") {
			style.Border, style.Fill = accent, selected
		}
	case "backupRadio", "backupRadioChecked":
		style.Width, style.Height, style.BorderWidth, style.Border = 13, 13, 1, muted
		if widget == "backupRadioChecked" {
			style.BorderWidth, style.Border = 4, accent
		}
	case "backupInput", "backupInputFocus":
		surface()
		style.Height, style.Font, style.Padding = 30, "monospace", [4]float32{6, 8, 6, 8}
		style.UsesMonoFont = true
		if widget == "backupInputFocus" {
			style.Border = accent
		}
	case "backupPathRow":
		style.ColumnGap = 8
	case "backupBrowse", "backupBrowseHover":
		style = DefaultWidgetStyle("settingsDataAction", palette)
		style.Size, style.Uppercase = 10, true
	case "backupError":
		style.Text = rgba(ui.LightTheme().Danger)
	case "backupReplaceConfirmation", "backupReplaceConfirmationChecked":
		style.MinHeight, style.MarginBottom, style.ColumnGap, style.Size = 32, 12, 8, 12
	case "backupReplaceMark", "backupReplaceMarkChecked", "backupReplaceMarkHover":
		surface()
		style.Width, style.Height, style.Radius, style.Size = 17, 17, min(4, style.Radius), 12
		style.Font, style.Text = "sans-serif", [4]uint8{255, 255, 255, 255}
		if widget != "backupReplaceMark" {
			style.Border = accent
		}
		if widget == "backupReplaceMarkChecked" {
			style.Fill = accent
		}
	case "backupPreview":
		surface()
		style.Height = 150
	case "backupPreviewRow":
		style.Text, style.Border = muted, border
		style.Padding = [4]float32{4, 8, 4, 8}
		style.BorderWidths = &[4]float32{0, 0, 1, 0}
	case "backupActions":
		style.MarginTop, style.ColumnGap = 4, 8
	case "backupCancel", "backupCancelHover", "backupSubmit", "backupSubmitHover":
		surface()
		style.Height, style.Weight, style.Uppercase = 34, 700, true
		style.Padding = [4]float32{8, 16, 8, 16}
		if strings.HasPrefix(widget, "backupSubmit") {
			style.Fill, style.Border, style.Text = accent, accent, [4]uint8{255, 255, 255, 255}
		}
		if strings.HasSuffix(widget, "Hover") {
			style.Fill = rgba(styleColor(style.Fill).Mix(ui.Hex(palette.Selected), 0.5))
		}
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
	case "tagSearchInput", "tagRailSearchInput", "tagPickerSearchInput":
		style = DefaultWidgetStyle("characterInput", palette)
		style.Size, style.Height, style.Padding = 12, 30, [4]float32{5, 7, 5, 28}
		if widget == "tagRailSearchInput" {
			style.Size, style.Padding = 11, [4]float32{6, 4, 6, 25}
		}
		if widget == "tagSearchInput" {
			style.Height, style.Padding = 32, [4]float32{6, 27, 6, 29}
		}
	case "tagSearchInputPlaceholder", "tagRailSearchInputPlaceholder", "tagPickerSearchInputPlaceholder":
		style = DefaultWidgetStyle("characterInputPlaceholder", palette)
	case "tagSearchInputIcon", "tagRailSearchInputIcon", "tagPickerSearchInputIcon":
		style = DefaultWidgetStyle("characterInputIcon", palette)
		style.Width, style.Height, style.Left = 13, 13, 9
		if widget == "tagRailSearchInputIcon" {
			style.Left = 8
		}
		if widget == "tagSearchInputIcon" {
			style.Width, style.Height = 14, 14
		}
	case "tagSearchClearIcon":
		style.Width, style.Height = 8, 13
	case "tagSearchClear":
		style = DefaultWidgetStyle("tagToolbarButton", palette)
		style.Width, style.Height = 23, 23
	case "privacyRulesHeading":
		style.Padding[0], style.Padding[2] = 8, 8
	case "privacyRuleCard":
		style.Padding[0], style.Padding[2] = 12, 12
		style.Border = border
		style.BorderWidths = &[4]float32{1, 0, 0, 0}
	case "privacyRuleAction", "privacyRuleCancel":
		style = DefaultWidgetStyle("settingsDataAction", palette)
		style.Height, style.Padding = 32, [4]float32{6, 12, 6, 12}
	case "privacyRuleTitle", "privacyRulesHeadingLabel":
		style.Weight = palette.EmphasisWeight
	case "privacyRuleSource", "privacyRuleScope":
		style.Text, style.Size = muted, 12
	case "privacyDetectionOptions":
		style.ColumnGap, style.RowGap, style.Margin[3] = 8, 8, 30
	case "privacyDetectionOption":
		style.ColumnGap = 8
	case "privacyDetectionLabel":
		style.Size = 12
	case "privacyPatternsContainer":
		style.Padding[3] = 30
	case "privacyPatternsField", "privacyPatternsFieldFocus":
		base := "textField"
		if widget == "privacyPatternsFieldFocus" {
			base = "textFieldFocus"
		}
		style = DefaultWidgetStyle(base, palette)
		style.Height, style.MinHeight, style.Radius = 80, 80, 0
		style.Padding = [4]float32{8, 8, 8, 8}
	case "privacySourceExclusion", "privacySourceExclusionFocus":
		base := "textField"
		if widget == "privacySourceExclusionFocus" {
			base = "textFieldFocus"
		}
		style = DefaultWidgetStyle(base, palette)
		style.Height, style.Padding = 88, [4]float32{8, 8, 8, 8}
	case "tagToolbar":
		style.Height, style.ColumnGap = 30, 2
		style.Padding = [4]float32{0, 7, 0, 3}
	case "tagBatchToolbar":
		style.ColumnGap, style.RowGap = 6, 6
		style.Padding = [4]float32{0, 7, 6, 3}
	case "tagBatchSelect", "tagBatchDelete":
		surface()
		style.Height, style.Size, style.Weight = 27, 12, 500
		style.Padding = [4]float32{3, 7, 3, 7}
		if widget == "tagBatchDelete" {
			style.Text = rgba(ui.LightTheme().Danger)
		}
	case "tagBatchCount":
		style.Size = 12
	case "tagToolbarTitle":
		style.Size, style.Weight = 12, 600
	case "tagToolbarTitleIcon":
		style.Width, style.Height = 14, 14
	case "tagPickerTrigger":
		style = DefaultWidgetStyle("headerButton", palette)
		style.Size, style.Padding = 12, [4]float32{3, 6, 3, 6}
	case "tagToolbarButton", "tagToolbarButtonActive":
		style = DefaultWidgetStyle("headerButton", palette)
		style.Width, style.Height = 27, 27
		style.Padding = [4]float32{}
		if widget == "tagToolbarButtonActive" {
			style.Fill, style.Border = selected, accent
		}
	case "tagSuggestions":
		surface()
		style.Padding, style.MarginTop = [4]float32{6, 6, 6, 6}, 5
	case "tagSuggestionHeading":
		style.Size, style.Text, style.Padding = 11, muted, [4]float32{3, 7, 7, 7}
	case "tagSuggestionCount":
		style.Size, style.Text = 10, muted
	case "tagSuggestionRow":
		style.Height, style.Padding = 36, [4]float32{1, 0, 1, 0}
	case "tagSuggestionOption", "tagSuggestionOptionActive":
		style.Size, style.Height, style.ColumnGap = 12, 34, 9
		style.Radius, style.BorderWidth, style.Padding = min(8, float32(palette.Radius)), 1, [4]float32{0, 9, 0, 9}
		if widget == "tagSuggestionOptionActive" {
			style.Fill, style.Border = selected, accent
		}
	case "tagSuggestionDot":
		style.Width, style.Height, style.Radius = 8, 8, 4
	case "tagSuggestionStatus":
		style.Size, style.Text, style.Padding = 12, muted, [4]float32{15, 8, 15, 8}
	case "tagSuggestionRetry":
		style.Size, style.Text, style.Padding = 11, rgba(ui.Hex("#c22d38")), [4]float32{5, 7, 5, 7}
	case "settingsDataPath", "settingsDataPathHover", "settingsDataAction", "settingsDataActionHover":
		surface()
		style.Height, style.Padding, style.Size = 30, [4]float32{5, 9, 5, 9}, 12
		style.ColumnGap = 6
		if widget == "settingsDataActionHover" {
			style.Fill, style.Border = selected, accent
		} else if widget != "settingsDataAction" {
			style.Font, style.Weight, style.UsesMonoFont = "monospace", 700, true
			if widget == "settingsDataPathHover" {
				style.Fill = selected
			}
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
		style.Radius = 9
		style.Padding, style.ColumnGap = [4]float32{2, 2, 2, 2}, 2
	case "settingsChoiceCards":
		style.ColumnGap = 8
	case "settingsSegment", "settingsSegmentHover", "settingsSegmentActive", "settingsChoiceCard", "settingsChoiceCardHover", "settingsChoiceCardActive":
		surface()
		style.Height, style.Size, style.Weight = 36, 12, palette.EmphasisWeight
		style.Padding = [4]float32{0, 12, 0, 12}
		if strings.HasPrefix(widget, "settingsSegment") {
			style.Height, style.BorderWidth, style.Fill = 30, 0, [4]uint8{}
			style.Radius = 6
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
	case "settingsSwitchDisabled":
		style.Opacity = 0.56
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
	case "selectSearchQuery":
		style.Size = 12
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
	case "selectOption", "selectSelected", "selectOptionFocused", "selectOptionDisabled", "historyItem", "historyEmpty":
		style.Height, style.Padding = 32, [4]float32{0, 8, 0, 8}
		style.ColumnGap = 8
		if widget == "selectSelected" || widget == "selectOptionFocused" {
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
	case "historyContent", "historyContentCompact":
		style.Font, style.UsesMonoFont = "monospace", true
		style.LineHeight, style.MinHeightEm, style.MaxLines = 1.35, 1.35, 4
		if widget == "historyContentCompact" {
			style.LineHeight, style.MaxLines, style.MaxHeight = 1.3, 2, 40
		}
	case "historyTags", "historyTagsCompact":
		style.ColumnGap, style.MarginTop = 4, 2
		if widget == "historyTagsCompact" {
			style.Right, style.Bottom, style.MaxWidthPercent, style.MarginTop, style.Opacity = 4, 2, 60, 0, 0.85
		}
	case "historyTag", "historyTagCompact":
		style.Text, style.Size, style.Weight = [4]uint8{255, 255, 255, 255}, 10, palette.EmphasisWeight
		style.Radius, style.Padding, style.LineHeight = 5, [4]float32{2, 6, 2, 6}, 1.4
		if widget == "historyTagCompact" {
			style.MinHeight, style.Padding, style.LineHeight = 16, [4]float32{0, 4, 0, 4}, 1
		}
	case "historyRichContent", "historyRichContentCompact":
		style.LineHeight, style.MaxHeight, style.MarginTop = 1.4, 64, 4
		if widget == "historyRichContentCompact" {
			style.MaxHeight = 40
		}
	case "historyOCRBadgeDone", "historyOCRBadgeProcessing", "historyOCRBadgeFailed", "historyOCRBadgeUnsupported":
		style.Fill, style.Text = muted, [4]uint8{255, 255, 255, 255}
		style.Size, style.Radius, style.Padding = 9, 3, [4]float32{1, 4, 1, 4}
		if widget == "historyOCRBadgeDone" {
			style.Fill = accent
		} else if widget == "historyOCRBadgeFailed" {
			style.Fill = [4]uint8{191, 54, 50, 255}
		}
	case "historyOCRSection":
		style.Border, style.BorderWidths = border, &[4]float32{1, 0, 0, 0}
		style.MarginTop, style.Padding = 4, [4]float32{4, 0, 0, 0}
	case "historyOCRButton":
		style.Text, style.Size, style.ColumnGap = muted, 9, 4
		style.Height, style.Padding = 14, [4]float32{1, 4, 1, 4}
	case "historyOCRContent":
		style.Fill, style.Text = [4]uint8{128, 128, 128, 13}, muted
		style.Font, style.UsesMonoFont = "monospace", true
		style.Size, style.LineHeight, style.MaxHeight = 11, 1.4, 80
		style.MarginTop, style.Radius, style.Padding = 2, 3, [4]float32{2, 4, 2, 4}
	case "historyConcealed":
		style.Size, style.Text, style.Padding = 12, muted, [4]float32{12, 0, 12, 0}
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
		style.Size, style.LineHeight = 13, 1.35
	case "attachmentNameCompact":
		style.Height = 17
		style.Size, style.LineHeight = 13, 1.3
	case "attachmentIcon":
		style.Text = muted
	case "attachmentImage", "attachmentImageCompact":
		style.Border, style.BorderWidth, style.Radius = border, 1, 4
		style.MaxHeight = 100
		if widget == "attachmentImageCompact" {
			style.MinWidth, style.MaxHeight = 44, 44
		}
	case "attachmentImagePending", "attachmentImageCompactPending":
		style = DefaultWidgetStyle(strings.TrimSuffix(widget, "Pending"), palette)
		style.Width, style.Height = 178, 100
		if widget == "attachmentImageCompactPending" {
			style.Width, style.Height = 78, 44
		}
	case "attachmentImageUnavailable", "attachmentImageCompactUnavailable":
		style = DefaultWidgetStyle(strings.TrimSuffix(widget, "Unavailable"), palette)
		style.MinHeight, style.Size, style.ColumnGap = 64, 11, 6
		style.Text, style.Padding = muted, [4]float32{6, 6, 6, 6}
		if widget == "attachmentImageCompactUnavailable" {
			style.MinHeight = 44
		}
	case "attachmentImageMissing", "attachmentImageCompactMissing":
		style = DefaultWidgetStyle(strings.TrimSuffix(widget, "Missing"), palette)
		style.Height, style.Size, style.RowGap, style.Fill, style.Text = style.MaxHeight, 12, 8, panel, muted
	case "previewToolbar":
		style.Padding, style.Border = [4]float32{4, 8, 4, 8}, border
		style.BorderWidths = &[4]float32{0, 0, 1, 0}
	case "previewToolbarTitle":
		style.Size = 12
	case "previewOpacityTrack":
		style.Height, style.Radius, style.BorderWidth = 6, 4, 1
		style.Fill, style.Border = rgba(ui.Hex(palette.Background)), border
	case "previewOpacityThumb":
		style.Width, style.Height, style.Radius, style.BorderWidth = 14, 14, 4, 2
		style.Fill, style.Border = panel, border
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
