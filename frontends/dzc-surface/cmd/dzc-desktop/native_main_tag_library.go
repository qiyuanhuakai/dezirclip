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
	contents := func() {
		n.tagLibraryControls(c, kit, tags)
		if tags.selected == "" {
			return
		}
		field := kit.TextField(c, &tags.query, n.text("tag_content_search", "Search tagged entries…")).
			FillWidth().Placeholder(n.text("tag_content_search", "Search tagged entries…")).OnTextKey(n.searchKey)
		if n.focusSearch {
			field.Focus()
			n.focusSearch = false
		}
		*queryFocused = field.Focused()
		n.editQuery(field.Composing())
		n.tagBatchToolbar(c, kit, tags)
		n.historyView(c, kit)
	}
	width, _ := c.Size()
	if width < nativeSplitWidth {
		ui.Column(c).FillWidth().Grow(1).MinHeight(0).Gap(5).Children(contents)
		return
	}
	ui.Row(c).FillWidth().Grow(1).MinHeight(0).Gap(8).Padding(0, 8, 8).AlignItems(ui.Stretch).Children(func() {
		ui.Column(c).Width(156).Shrink(0).MinHeight(0).Gap(4).Children(func() {
			ui.Row(c).FillWidth().AlignItems(ui.Center).Children(func() {
				kit.SettingsLabel(c, n.text("tags")).Grow(1)
				if kit.TagToolbarButton(c, uikit.HeaderAction{Icon: "add", Label: n.text("tag_create"), Disabled: n.mutating}).Clicked() {
					n.startTagDialog(tags, "create")
				}
			})
			kit.TextField(c, &tags.nameQuery, n.text("tag_search_label", "Find a tag")).
				FillWidth().Placeholder(n.text("tag_search_label", "Find a tag…"))
			ui.Scroll(c).FillWidth().Grow(1).MinHeight(0).Children(func() {
				sections := make([]uikit.SettingsSection, 0, len(n.tagCatalog.names))
				for _, name := range n.tagCatalog.names {
					if strings.Contains(strings.ToLower(name), strings.ToLower(strings.TrimSpace(tags.nameQuery))) {
						sections = append(sections, uikit.SettingsSection{ID: name, Label: fmt.Sprintf("%s (%d)", name, n.tagCatalog.counts[name])})
					}
				}
				kit.SettingsRail(c, n.text("tags"), tags.selected, sections, func(id string) {
					tags.selected = id
					n.editQuery(false)
				})
			})
		})
		ui.Column(c).Grow(1).MinWidth(0).FillHeight().MinHeight(0).Gap(5).Children(contents)
	})
}

func (n *nativeMain) tagLibraryControls(c *ui.Context, kit uikit.Kit, tags *nativeTagLibrary) {
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
		options := make([]string, 0, len(catalog.names))
		names := make(map[string]string, len(catalog.names))
		current := ""
		for _, name := range catalog.names {
			label := fmt.Sprintf("%s (%d)", name, catalog.counts[name])
			options = append(options, label)
			names[label] = name
			if name == tags.selected {
				current = label
			}
		}
		picker, changed := kit.TagPicker(c, &current, n.text("tag_name", "Tag"), n.text("native_select_tag"), options, len(options) == 0)
		picker.Width(0).Grow(1).MinWidth(72)
		if changed {
			tags.selected = names[current]
		}
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
