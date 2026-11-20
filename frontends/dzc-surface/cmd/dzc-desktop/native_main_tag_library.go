package main

import (
	"context"
	"fmt"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type nativeTagLibrary struct {
	selected, query, sort, nameQuery string
	railCollapsed, focusRailSearch   bool
	dialog                           *nativeTagDialog
	batch                            nativeTagBatch
	reveal                           map[nativeCardKey]bool
	revealGeneration                 uint64
}

type nativeTagDialog struct {
	action, tag, value, message string
	open, focus                 bool
}

func (n *nativeMain) historyQuery() string {
	if n.tagLibrary != nil {
		return n.tagLibrary.query
	}
	return n.query
}

func (n *nativeMain) hasHistoryDialog() bool {
	return n.tags != nil || n.editor != nil || n.transform != nil || n.qr != nil || n.privacyDialog != nil ||
		n.tagLibrary != nil && (n.tagLibrary.dialog != nil || n.tagLibrary.batch.pending != nil)
}

func (n *nativeMain) openTagLibrary() {
	if n.session == nil || n.history.state.ServiceInstanceId == "" || n.mutating {
		return
	}
	n.tagLibrary = &nativeTagLibrary{sort: "time"}
	n.tagSuggestions.dismiss()
	n.editQuery(false)
	n.loadTagCatalog()
}

func (n *nativeMain) closeTagLibrary() {
	n.tagLibrary = nil
	n.editQuery(false)
}

func (n *nativeMain) tagLibraryView(c *ui.Context, kit uikit.Kit, queryFocused *bool) {
	tags := n.tagLibrary
	const gap, padding = 8, 8
	width, _ := c.Size()
	historyWidth := width
	if width >= nativeSplitWidth {
		historyWidth -= nativeTagRailWidth(tags.railCollapsed) + gap + 2*padding
	}
	contents := func() {
		n.tagLibraryControls(c, kit, tags, queryFocused)
		field := kit.TagSearch(c, uikit.TagSearchOptions{
			Variant: uikit.TagContentsSearch, Query: &tags.query, Label: n.text("tag_content_search", "Search tagged entries…"),
			ClearLabel: n.text("tag_clear_search", "Clear search"), OnKey: n.searchKey, Disabled: tags.selected == "",
		})
		if tags.selected == "" {
			return
		}
		if n.focusSearch {
			field.Focus()
			n.focusSearch = false
		}
		*queryFocused = *queryFocused || field.Focused()
		n.editQuery(field.Composing())
		n.tagBatchToolbar(c, kit, tags)
		n.historyView(c, kit, historyWidth)
	}
	if width < nativeSplitWidth {
		ui.Column(c).FillWidth().Grow(1).MinHeight(0).Gap(5).Children(contents)
		return
	}
	ui.Row(c).FillWidth().Grow(1).MinHeight(0).Gap(gap).Padding(0, padding, padding).AlignItems(ui.Stretch).Children(func() {
		n.tagLibraryRail(c, kit, tags, queryFocused)
		ui.Column(c).Grow(1).MinWidth(0).FillHeight().MinHeight(0).Gap(5).Children(contents)
	})
}

func nativeTagRailWidth(collapsed bool) float32 {
	if collapsed {
		return 42
	}
	return 150
}

func (n *nativeMain) tagLibraryRail(c *ui.Context, kit uikit.Kit, tags *nativeTagLibrary, queryFocused *bool) {
	width := nativeTagRailWidth(tags.railCollapsed)
	icon, toggle := "back", n.text("tag_collapse_navigation")
	if tags.railCollapsed {
		icon, toggle = "forward", n.text("tag_expand_navigation")
	}
	ui.Column(c).Width(width).Shrink(0).MinHeight(0).Gap(4).Children(func() {
		ui.Row(c).FillWidth().AlignItems(ui.Center).Justify(ui.SpaceBetween).Children(func() {
			if kit.TagToolbarButton(c, uikit.HeaderAction{Icon: icon, Label: toggle}).Clicked() {
				tags.railCollapsed = !tags.railCollapsed
				c.Invalidate()
			}
			if !tags.railCollapsed {
				n.tagRailAdd(c, kit, tags)
			}
		})
		if tags.railCollapsed {
			if kit.TagToolbarButton(c, uikit.HeaderAction{Icon: "search", Label: n.text("tag_search_label")}).Clicked() {
				tags.railCollapsed, tags.focusRailSearch = false, true
				n.focusList = false
				c.Invalidate()
			}
		} else {
			field := kit.TagSearch(c, uikit.TagSearchOptions{Variant: uikit.TagRailSearch,
				Query: &tags.nameQuery, Label: n.text("tag_search_label", "Find a tag")})
			if tags.focusRailSearch {
				tags.focusRailSearch = !field.Focused()
				if tags.focusRailSearch {
					field.Focus()
					c.Invalidate()
				}
			}
			*queryFocused = field.Focused()
		}
		ui.Scroll(c).FillWidth().Grow(1).MinHeight(0).Children(func() {
			ui.Column(c).FillWidth().Gap(4).Children(func() {
				for _, name := range n.tagCatalog.names {
					if !uikit.MatchesTagName(name, tags.nameQuery) {
						continue
					}
					choice := uikit.TagChoice{ID: name, Label: name, Count: n.tagCatalog.counts[name]}
					if kit.TagRailChoice(c, choice, tags.selected == name, tags.railCollapsed).Clicked() {
						tags.selected = name
						n.editQuery(false)
					}
				}
			})
		})
		if tags.railCollapsed {
			n.tagRailAdd(c, kit, tags)
		}
	})
}

func (n *nativeMain) tagRailAdd(c *ui.Context, kit uikit.Kit, tags *nativeTagLibrary) {
	if kit.TagToolbarButton(c, uikit.HeaderAction{Icon: "add", Label: n.text("tag_create"), Disabled: n.mutating}).Clicked() {
		n.startTagDialog(tags, "create")
	}
}

func (n *nativeMain) tagLibraryControls(c *ui.Context, kit uikit.Kit, tags *nativeTagLibrary, queryFocused *bool) {
	catalog := &n.tagCatalog
	if !catalog.fresh && !catalog.failed {
		n.loadTagCatalog()
	}
	width, _ := c.Size()
	selected := tags.selected != ""
	if tags.reveal != nil && tags.revealGeneration != n.history.generation {
		n.clearProtectedRows()
	}
	compact := n.preferences["app.compact_mode"] == "true"
	layoutIcon, layoutLabel := "collapse", n.text("tag_mode_simple", "Compact view")
	if compact {
		layoutIcon, layoutLabel = "list", n.text("tag_mode_detailed", "Detailed view")
	}
	manageIcon, manageLabel := "manage", n.text("tag_manage")
	if tags.batch.enabled {
		manageIcon, manageLabel = "done", n.text("tag_manage_done")
	}
	revealIcon, revealLabel := "reveal", n.text("tag_reveal_all", "Reveal protected items")
	if tags.reveal != nil {
		revealIcon, revealLabel = "conceal", n.text("tag_hide_all", "Hide protected items")
	}
	kit.TagToolbar(c, func() {
		if width >= nativeSplitWidth {
			label := tags.selected
			if label == "" {
				label = n.text("tags")
			}
			kit.SettingsLabel(c, label).Grow(1).MinWidth(0).SingleLine()
			return
		}
		options := make([]uikit.TagChoice, 0, len(catalog.names))
		for _, name := range catalog.names {
			options = append(options, uikit.TagChoice{ID: name, Label: name, Count: catalog.counts[name]})
		}
		var create func()
		if !n.mutating {
			create = func() { n.startTagDialog(tags, "create") }
		}
		_, pickerOpen := kit.TagNavigation(c, uikit.TagNavigationOptions{
			Label: n.text("tags"), SearchLabel: n.text("tag_search_label"), CreateLabel: n.text("tag_create"),
			EmptyLabel: n.text("tag_no_matching_tags"), Selected: tags.selected, Choices: options, OnCreate: create,
			OnChoose: func(name string) {
				tags.selected = name
				n.editQuery(false)
			},
		})
		*queryFocused = *queryFocused || pickerOpen
	}, []uikit.HeaderAction{
		{Icon: layoutIcon, Label: layoutLabel, Active: compact, Disabled: n.mutating, Run: func() {
			n.saveSetting("app.compact_mode", fmt.Sprint(!compact))
		}},
		{Icon: revealIcon, Label: revealLabel, Active: tags.reveal != nil,
			Disabled: !selected || n.mutating || n.reading || n.failed || tags.batch.enabled || len(n.history.rows) == 0,
			Run: func() {
				n.clearProtectedRows()
				if revealIcon == "reveal" {
					tags.reveal, tags.revealGeneration = make(map[nativeCardKey]bool), n.history.generation
				}
			}},
		{Icon: manageIcon, Label: manageLabel, Active: tags.batch.enabled, Disabled: !selected || n.mutating, Run: func() {
			n.clearProtectedRows()
			tags.batch = nativeTagBatch{enabled: !tags.batch.enabled, generation: n.history.generation}
			n.resetPreviewSelection()
		}},
		{Icon: "add", Label: n.text("add_item", "Add entry"), Disabled: !selected || n.mutating || tags.batch.enabled,
			Run: func() { n.addHistoryEntry(tags.selected) }},
	}, func() {
		more := kit.TagToolbarButton(c, uikit.HeaderAction{Icon: "more", Label: n.text("tag_actions", "Tag actions"), Disabled: n.mutating})
		if more.Clicked() {
			n.menu.Open(more, func() []uikit.MenuAction {
				sort, sortLabel := "count", n.text("sort_usage", "Most used")
				if tags.sort == "count" {
					sort, sortLabel = "time", n.text("sort_time")
				}
				edit := tags.selected != "" && !nativeProtectedTag(tags.selected) && !tags.batch.enabled
				return []uikit.MenuAction{
					{Label: n.text("tag_create", "New tag"), Run: func() { n.startTagDialog(tags, "create") }},
					{Label: sortLabel, Disabled: tags.selected == "", Run: func() { tags.sort = sort; n.editQuery(false) }},
					{Label: n.text("tag_rename", "Rename"), Disabled: !edit, Run: func() { n.startTagDialog(tags, "rename") }},
					{Label: n.text("tag_remove", "Remove"), Disabled: !edit, Run: func() { n.startTagDialog(tags, "remove") }},
				}
			})
		}
	})
	if catalog.failed {
		if kit.Button(c, uikit.ButtonOptions{Label: n.text("common_retry", "Retry loading tags")}).Clicked() {
			n.loadTagCatalog()
		}
	} else if !selected {
		label := n.text("tag_library_empty", "No tags")
		if catalog.loading {
			label = n.text("common_loading", "Loading tags…")
		}
		ui.Text(c, label).TextColor(c.Theme().TextMuted).Padding(8)
	}
}

func (n *nativeMain) startTagDialog(tags *nativeTagLibrary, action string) {
	n.resetPreviewSelection()
	tags.dialog = &nativeTagDialog{action: action, tag: tags.selected, open: true, focus: true}
	if action == "rename" {
		tags.dialog.value = tags.selected
	}
}

func nativeProtectedTag(name string) bool {
	switch strings.ToLower(strings.TrimSpace(name)) {
	case "sensitive", "password", "密码":
		return true
	}
	return false
}

func (n *nativeMain) tagLibraryDialog(c *ui.Context, kit uikit.Kit, tags *nativeTagLibrary) {
	dialog := tags.dialog
	title, confirm := n.text("tag_create"), n.text("native_create")
	switch dialog.action {
	case "rename":
		title, confirm = n.text("tag_rename"), n.text("common_save")
	case "remove":
		title, confirm = n.text("tag_remove"), n.text("tag_remove", "Remove")
	}
	kit.Dialog(c, &dialog.open, title, n.text("common_close"), n.mutating, func() {
		switch dialog.action {
		case "remove":
			message := n.text("native_remove_tag_message")
			ui.Text(c, strings.ReplaceAll(message, "{tag}", dialog.tag)).Wrap()
		default:
			input := kit.TagNameField(c, &dialog.value, n.text("tag_name"), n.mutating)
			if dialog.focus {
				input.Focus()
			}
		}
		dialog.focus = false
		if dialog.message != "" {
			ui.Text(c, dialog.message).TextColor(c.Theme().Danger).Wrap()
		}
		ui.Row(c).Justify(ui.End).Children(func() {
			disabled := n.mutating || dialog.action != "remove" && strings.TrimSpace(dialog.value) == ""
			if n.mainKit().Button(c, uikit.ButtonOptions{Label: confirm, Disabled: disabled}).Clicked() {
				n.commitTagDialog(tags, dialog)
			}
		})
	})
	if !dialog.open {
		tags.dialog = nil
	}
}

func (n *nativeMain) commitTagDialog(tags *nativeTagLibrary, dialog *nativeTagDialog) {
	if n.session == nil || n.mutating || n.tagLibrary != tags {
		return
	}
	name := strings.TrimSpace(dialog.value)
	if name == "" && dialog.action != "remove" {
		return
	}
	if dialog.action == "create" || dialog.action == "rename" {
		if utf8.RuneCountInString(name) > 80 {
			dialog.message = n.text("native_tag_length")
			return
		}
		if dialog.action == "rename" && nativeProtectedTag(name) {
			dialog.message = n.text("tag_protected", "Choose a name outside the reserved sensitive tags.")
			return
		}
		if dialog.action == "create" {
			for _, existing := range n.tagCatalog.names {
				if strings.EqualFold(name, existing) {
					tags.selected, tags.nameQuery, tags.dialog = existing, "", nil
					n.editQuery(false)
					return
				}
			}
		}
	}
	session, state := n.session, n.history.state
	action, oldName := dialog.action, dialog.tag
	n.mutating, dialog.message = true, ""
	go func() {
		ctx, cancel := context.WithTimeout(session, 5*time.Second)
		defer cancel()
		var err error
		switch action {
		case "create":
			err = n.library.EditTag(ctx, action, name, "", state.PrivacyEpoch)
		default:
			err = n.library.EditTag(ctx, action, oldName, name, state.PrivacyEpoch)
		}
		n.update(session, func() {
			n.mutating = false
			if n.tagLibrary != tags || tags.dialog != dialog {
				return
			}
			if err != nil {
				dialog.message = n.text("tag_action_failed", "Could not apply the tag change.")
				return
			}
			switch action {
			case "create", "rename":
				tags.selected, tags.nameQuery = name, ""
			case "remove":
				tags.selected, tags.query = "", ""
			}
			tags.dialog = nil
			n.editQuery(false)
			n.tagCatalog.invalidate()
			n.loadTagCatalog()
		})
	}()
}
