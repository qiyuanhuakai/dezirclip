package main

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

func (n *nativeMain) mainKit() uikit.Kit {
	return n.preset().Kit().WithInterfaceFont(nativeInterfaceFont(n.preferences))
}

func (n *nativeMain) view(c *ui.Context) {
	preset := n.preset()
	kit := n.mainKit()
	kit.Apply(c)
	var surfaceOpacity *int
	tint := preset.SurfaceTint()
	if tint != "" {
		opacity := opacityPreference(n.preferences["app.surface_opacity"])
		surfaceOpacity = &opacity
		palette := *c.Theme()
		// Paint the shell only once, below the controls.
		palette.Background = ui.Color{}
		c.SetTheme(&palette)
	}
	n.list.List.Selected = &n.history.selected
	n.list.List.Key = func(index int) any {
		if index >= 0 && index < len(n.history.rows) {
			return n.history.rows[index].Id
		}
		return nil
	}
	n.list.List.Label = func(index int) string {
		if index >= 0 && index < len(n.history.rows) {
			return nativeCardSummary(n.history.rows[index], n.language())
		}
		return ""
	}
	shell := ui.Box(c).Fill().Clip()
	queryFocused := false
	shell.Children(func() {
		n.drawBackground(c, preset.Tokens().Radius)
		main := ui.Column(c).Fill().Gap(6).Background(c.Theme().Background)
		if n.preferences["app.show_app_border"] != "false" {
			main.Border(1, c.Theme().Border)
		}
		main.Children(func() {
			queryComposing := false
			pinIcon := "pin"
			if n.pinned {
				pinIcon = "unpin"
			}
			pin := uikit.HeaderAction{Icon: pinIcon, Label: n.text("native_keep_on_top"), Active: n.pinned, Run: n.togglePin, Disabled: n.mutating}
			close := uikit.HeaderAction{Icon: "hide", Label: n.text("hide"), Run: n.hideWindow}
			header := uikit.HeaderOptions{Title: "DezirClip", Actions: []uikit.HeaderAction{pin}, Draggable: true}
			if n.showSettings {
				header.Title = n.text("settings")
				close.Run = n.closeSettings
				close.Label = n.text("common_close")
				close.Disabled = n.profile.moving || n.profile.restartPending
				header.Back = &uikit.HeaderAction{Icon: "back", Label: n.text("native_back"),
					Disabled: close.Disabled, Run: n.closeSettings}
			} else if n.showEmoji {
				header.Title = n.text("emoji_tab")
				header.Back = &uikit.HeaderAction{Icon: "back", Label: n.text("native_back"), Run: n.closeEmoji, Disabled: n.mutating}
			} else if n.tagLibrary != nil {
				header.Title = n.text("tags")
				header.Back = &uikit.HeaderAction{Icon: "back", Label: n.text("native_back"), Run: n.closeTagLibrary, Disabled: n.mutating}
			} else {
				header.Actions = append(header.Actions, uikit.HeaderAction{Icon: "clear", Label: n.text("clear_history"),
					Disabled: n.session == nil || n.mutating || n.failed, Run: n.openClearHistory})
				if n.preferences["app.tag_manager_enabled"] != "false" {
					header.Actions = append(header.Actions, uikit.HeaderAction{
						Icon: "tag", Label: n.text("tags"), Run: n.openTagLibrary, Disabled: n.mutating,
					})
				}
				if n.preferences["app.emoji_panel_enabled"] == "true" {
					header.Actions = append(header.Actions, uikit.HeaderAction{
						Icon: "emoji", Label: n.text("emoji_tab"), Run: n.openEmoji, Disabled: n.mutating,
					})
				}
				header.Actions = append(header.Actions, uikit.HeaderAction{Icon: "settings", Label: n.text("settings"), Run: n.openSettings})
				if n.focusSearch {
					n.temporarySearch = true
				}
				if n.preferences["app.show_search_box"] != "false" || n.temporarySearch || n.query != "" {
					header.Query, header.Placeholder, header.Filter = &n.query, n.text("search_placeholder", "Search clipboard…"), &n.kind
					header.QueryPopup = func(input ui.Element) { n.tagSuggestionsView(c, input) }
					header.Filters = []uikit.HeaderFilter{
						{ID: "text", Label: n.text("type_text")}, {ID: "image", Label: n.text("native_images")},
						{ID: "file", Label: n.text("native_files")}, {ID: "url", Label: n.text("type_url")},
						{ID: "code", Label: n.text("type_code")}, {ID: "video", Label: n.text("type_video")},
						{ID: "rich_text", Label: n.text("type_rich_text")},
					}
				}
			}
			header.Actions = append(header.Actions, close)
			if header.QueryPopup == nil {
				n.tagSuggestions.dismiss()
				n.tagSuggestions.focused = false
			}
			if header.Query != nil {
				header.FocusQuery = &n.focusSearch
				header.OnQueryEdit = func(composing bool) { queryComposing = composing }
				header.OnQueryKey = n.searchKey
				header.QueryFocus = func(focused bool) { queryFocused = focused }
			}
			kit.Header(c, header)
			if n.showSettings {
				n.settingsView(c, kit, surfaceOpacity)
			} else if n.showEmoji {
				n.emojiView(c)
			} else {
				if n.tagLibrary != nil {
					n.tagLibraryView(c, kit)
				} else {
					n.editQuery(queryComposing)
					n.historyView(c, kit)
				}
			}
			if n.notice != "" {
				ui.Text(c, n.notice).Padding(4, 8).TextColor(c.Theme().TextMuted)
			}
			if n.failed {
				ui.Row(c).Padding(8).AlignItems(ui.Center).Gap(8).Children(func() {
					ui.Text(c, n.text("native_service_unavailable")).Grow(1).TextColor(c.Theme().Danger)
					if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("common_retry")}).Clicked() {
						n.prepare(n.resume)
					}
				})
			}
		})
		if surfaceOpacity != nil {
			main.Background(ui.Hex(tint).Alpha(float32(*surfaceOpacity) / 100))
		}
	})
	if n.menu.Active() {
		kit.ContextMenu(c, &n.menu, n.text("common_close"))
	} else {
		n.historyShortcuts(c, queryFocused)
	}
	if n.clearHistoryOpen {
		kit.ConfirmDialog(c, &n.clearHistoryOpen, n.text("clear_confirm_title"), n.text("clear_confirm_message"),
			n.text("common_cancel"), n.text("common_confirm"), n.clearHistory)
	}
	if n.tags != nil {
		n.tagEditorView(c, kit, n.tags)
	}
	if tags := n.tagLibrary; tags != nil {
		if tags.dialog != nil {
			n.tagLibraryDialog(c, kit, tags)
		}
		if tags.batch.pending != nil {
			n.tagBatchDialog(c, kit, tags)
		}
	}
	if n.backup != nil {
		n.backupView(c, kit, n.backup)
	}
	if n.editor != nil {
		n.entryEditorView(c, kit, n.editor)
	}
	if n.transform != nil {
		n.transformView(c, kit, n.transform)
	}
	if n.qr != nil {
		n.qrView(c, kit, n.qr)
	}
	if n.privacyDialog != nil {
		n.privacyDialogView(c, kit, n.privacyDialog)
	}
	if n.privacyEditor != nil {
		n.privacyEditorView(c, kit, n.privacyEditor)
	}
	if n.shortcut != nil && n.shortcut.open {
		n.drawShortcutRecorder(c, kit, n.shortcut)
	}
	if n.showSettings && n.resetOpen {
		kit.ConfirmDialog(c, &n.resetOpen, n.text("reset_defaults"), n.text("reset_confirm"),
			n.text("common_cancel"), n.text("common_confirm"), n.confirmSettingsReset)
	}
	if n.showSettings && n.updater.confirm {
		kit.ConfirmDialog(c, &n.updater.confirm, n.text("updater_confirm_title"), n.text("updater_confirm_message"),
			n.text("common_cancel"), n.text("updater_confirm_install"), n.installUpdate)
	}
}

func (n *nativeMain) historyView(c *ui.Context, kit uikit.Kit) {
	if n.private {
		ui.Text(c, n.text("privacy_session_title", "Private session")).TextColor(c.Theme().TextMuted).Padding(4, 8)
	}
	generation := n.history.generation
	compact := n.preferences["app.compact_mode"] == "true"
	itemSize, tagSize := nativeItemSize.value(n.preferences), nativeTagSize.value(n.preferences)
	first, last := n.list.List.Visible()
	n.pruneProtectedRows(first, last)
	batch := n.activeTagBatch()
	reorderPins := batch == nil && n.canReorderVisiblePins()
	draggingPin := false
	var hovered protocol.DesktopEntry
	var hoverBounds ui.Rect
	list := kit.HistoryList(c, &n.list, uikit.HistoryListOptions{
		Count: len(n.history.rows), Label: n.text("native_clipboard_history"), EmptyLabel: n.text("no_records", "No clipboard entries"),
		LoadingLabel: n.text("common_loading", "Loading…"), MoreLabel: n.text("tag_load_more", "Load more…"), Loading: n.reading, HasMore: n.history.more && !n.failed,
		Compact:    compact,
		OnLoadMore: n.loadPage,
		Row: func(cell ui.ListRow) {
			if generation != n.history.generation || cell.Index >= len(n.history.rows) {
				return
			}
			row := n.history.rows[cell.Index]
			pinLabel := n.text("pin", "Pin entry")
			pinIcon := "pin"
			if row.IsPinned {
				pinLabel = n.text("unpin", "Unpin entry")
				pinIcon = "unpin"
			}
			actions := []uikit.HeaderAction{
				{Icon: pinIcon, Label: pinLabel, Active: row.IsPinned, Disabled: n.mutating, Run: func() { n.rowAction(row, generation, nativePin) }},
				{Icon: "hide", Label: n.text("delete", "Delete entry"), Disabled: n.mutating, Run: func() { n.rowAction(row, generation, nativeDelete) }},
			}
			summary := nativeCardSummary(row, n.language())
			protected := n.protectedBody(row)
			if batch != nil {
				protected = nil
			}
			if editableHistoryKind(row.ContentType) {
				actions = append(actions, uikit.HeaderAction{Icon: "edit", Label: n.text("edit_item"),
					Disabled: !n.canOpenHistoryDialog() || nativeConcealed(row) && protected == nil,
					Run:      func() { n.editHistoryEntry(row, generation) }})
			}
			if nativeConcealed(row) {
				actions = append(actions, n.protectedRowAction(row, generation))
				if protected != nil {
					summary = protected.summary
					if row.ContentType == "image" && row.Id > 0 {
						actions = append(actions, uikit.HeaderAction{Icon: "scan", Label: n.text("native_ocr_recognize"),
							Disabled: !n.eligibleOCR(row, generation) || n.private || n.ocrJob.status == "processing" || protected.ocrStatus == "processing",
							Run:      func() { n.recognizeOCR(row, generation) }})
					}
					if nativeTextToolEntry(row.ContentType) {
						actions = append(actions,
							uikit.HeaderAction{Icon: "qr", Label: n.text("native_qr_code"),
								Disabled: !n.canOpenHistoryDialog(), Run: func() { n.openQR(row, generation) }},
							uikit.HeaderAction{Icon: "transform", Label: n.text("native_transformations"),
								Disabled: !n.canOpenHistoryDialog(), Run: func() { n.openTransform(row, generation) }})
					}
				}
			}
			textRows := 4
			if compact {
				textRows = 1
			}
			if batch != nil {
				activate := func() { n.toggleTagBatchRow(batch, row.Id, generation) }
				label := n.text("tag_select_entry")
				kit.HistoryCard(cell.Context, uikit.HistoryCardOptions{
					Source: row.SourceApp, Time: time.UnixMilli(row.Timestamp).Format("15:04"), Summary: summary,
					Pinned: row.IsPinned, Selected: batch.selection.includes(row.Id), Compact: compact,
					TextRows: textRows, TextFont: n.preferences["app.font_mono"], TextSize: float32(itemSize),
					Tags: n.cardTags(row.Tags), TagSize: float32(tagSize), Activate: activate,
					Leading: func() {
						checked := batch.selection.includes(row.Id)
						if kit.Checkbox(cell.Context, &checked, "").Label(label + ": " + summary).
							Width(24).Shrink(0).Disabled(row.Id <= 0 || n.mutating).Changed() {
							activate()
						}
					},
				})
				return
			}
			card := kit.HistoryCard(cell.Context, uikit.HistoryCardOptions{
				Source: row.SourceApp, Time: time.UnixMilli(row.Timestamp).Format("15:04"), Summary: summary,
				Pinned: row.IsPinned, Selected: cell.Selected(), Compact: compact,
				Match:    n.history.filter.query,
				TextFont: n.preferences["app.font_mono"],
				TextSize: float32(itemSize),
				TextRows: textRows,
				TagSize:  float32(tagSize),
				Tags:     n.cardTags(row.Tags),
				Body:     n.cardBody(cell.Context, row, protected, compact, generation),
				Reorder:  n.pinReorderGrip(row, cell.Index, generation, reorderPins, func() { draggingPin = true }),
				Activate: func() {
					if n.fileGesture.takeClick(row.Id) {
						return
					}
					n.rowAction(row, generation, nativePaste)
					n.resetPreviewSelection()
				},
				Actions: actions,
			})
			if reorderPins && row.Id > 0 && row.IsPinned {
				draggingPin = n.pinDropTarget(cell.Context, card, row, generation) || draggingPin
			}
			n.entryMenu(card, row, generation)
			if nativeFileCard(row) && (!nativeConcealed(row) || protected != nil) {
				card.HandleInput(func(event ui.InputEvent) bool { return n.fileCardInput(event, row, generation) })
			}
			if card.Hovered() {
				hovered, hoverBounds = row, card.Bounds()
			}
			if cell.Selected() && n.keyboard {
				n.previewKeyboardRow(row, generation, card.Bounds())
			}
		},
	})
	list.TrackScroll(&n.historyScroll).HandleInput(func(event ui.InputEvent) bool {
		if batch != nil && n.tagBatchKey(batch, event) {
			return true
		}
		return n.historyWheel(event)
	})
	if reorderPins {
		_, over := ui.DragOver[nativePinSource](list)
		draggingPin = draggingPin || over
	}
	if draggingPin {
		n.hover.reset()
		n.closePreview()
	} else {
		n.hoverPreview(hovered, generation, hoverBounds)
	}
	if n.focusList {
		list.Focus()
		n.focusList = false
	}
	if list.Submitted() && n.history.selected >= 0 && n.history.selected < len(n.history.rows) {
		if batch != nil {
			n.toggleTagBatchRow(batch, n.history.rows[n.history.selected].Id, generation)
		} else {
			n.selectedAction(nativePaste)
		}
	}
	if first > 0 && !draggingPin && n.preferences["app.show_scroll_top_button"] != "false" {
		ui.Row(c).Padding(0, 8, 5).Justify(ui.End).Children(func() {
			if kit.ScrollToTopButton(c, n.text("scroll_to_top", "Scroll to top")).Clicked() {
				n.list.List.ScrollTo(0, ui.Start)
			}
		})
	}
}
