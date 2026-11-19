package main

import (
	"context"
	"encoding/json"
	"fmt"
	"sort"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type nativeTagLibrary struct {
	selected, query, sort, nameQuery string
	counts                           map[string]uint64
	names                            []string
	cancelRead                       context.CancelFunc
	read                             uint64
	loading, failed                  bool
	dialog                           *nativeTagDialog
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
		n.tagLibrary != nil && n.tagLibrary.dialog != nil
}

func (n *nativeMain) openTagLibrary() {
	if n.session == nil || n.history.state.ServiceInstanceId == "" || n.mutating {
		return
	}
	n.tagLibrary = &nativeTagLibrary{sort: "time"}
	n.editQuery(false)
	n.loadTagCatalog()
}

func (n *nativeMain) closeTagLibrary() {
	if n.tagLibrary.cancelRead != nil {
		n.tagLibrary.cancelRead()
	}
	n.tagLibrary = nil
	n.editQuery(false)
}

func (n *nativeMain) loadTagCatalog() {
	tags := n.tagLibrary
	if tags == nil || n.session == nil {
		return
	}
	if tags.cancelRead != nil {
		tags.cancelRead()
	}
	tags.read++
	tags.loading, tags.failed = true, false
	session, sequence, epoch := n.session, tags.read, n.history.state.PrivacyEpoch
	ctx, cancel := context.WithTimeout(session, 4*time.Second)
	tags.cancelRead = cancel
	go func() {
		defer cancel()
		raw, err := n.library.TagCounts(ctx, epoch)
		var counts map[string]uint64
		if err == nil {
			err = json.Unmarshal(raw, &counts)
		}
		names := make([]string, 0, len(counts))
		for name := range counts {
			names = append(names, name)
		}
		sort.Slice(names, func(i, j int) bool {
			left, right := strings.ToLower(names[i]), strings.ToLower(names[j])
			return left < right || left == right && names[i] < names[j]
		})
		n.update(session, func() {
			if n.tagLibrary != tags || tags.read != sequence {
				return
			}
			tags.cancelRead, tags.loading, tags.failed = nil, false, err != nil
			if err == nil {
				tags.counts, tags.names = counts, names
			}
		})
	}()
}

func (n *nativeMain) tagLibraryControls(c *ui.Context, kit uikit.Kit, tags *nativeTagLibrary) {
	width, _ := c.Size()
	pickers, actionsPerRow := 2, 4
	if width < 310 {
		pickers = 1
	}
	if width < 370 {
		actionsPerRow = 2
	}
	ui.Column(c).Padding(0, 8).Gap(5).Children(func() {
		ui.TextInput(c, &tags.nameQuery).FillWidth().Label(n.text("tag_search_label", "Find a tag")).Placeholder(n.text("tag_search_label", "Find a tag…"))
		ui.Grid(c).Columns(pickers).Width(300).MaxWidthPercent(100).Gap(5).Children(func() {
			options := []uikit.SelectOption{{Label: n.text("native_select_tag")}}
			search := strings.ToLower(strings.TrimSpace(tags.nameQuery))
			if name, ok := searchTagName(search); ok {
				search = name
			}
			for _, name := range tags.names {
				if strings.Contains(strings.ToLower(name), search) || name == tags.selected {
					options = append(options, uikit.SelectOption{
						Value: name, Label: fmt.Sprintf("%s (%d)", name, tags.counts[name]),
					})
				}
			}
			kit.Select(c, &tags.selected, n.text("tag_name", "Tag"), options, len(options) == 1)
			kit.Select(c, &tags.sort, n.text("native_sort_tag_entries"), []uikit.SelectOption{
				{Value: "time", Label: n.text("sort_time")}, {Value: "count", Label: n.text("sort_usage", "Most used")},
			}, tags.selected == "")
		})
		ui.Grid(c).Columns(actionsPerRow).MaxWidth(460).Gap(5).Children(func() {
			canEdit := tags.selected != "" && !nativeProtectedTag(tags.selected)
			actions := []struct {
				id, label string
				enabled   bool
			}{
				{"create", n.text("tag_create", "New tag"), true},
				{"rename", n.text("tag_rename", "Rename"), canEdit},
				{"remove", n.text("tag_remove", "Remove"), canEdit},
				{"add", n.text("add_item", "Add entry"), tags.selected != ""},
			}
			for _, action := range actions {
				if uikit.Button(c, uikit.ButtonOptions{Label: action.label, Disabled: n.mutating || !action.enabled}).Clicked() {
					if action.id == "add" {
						n.addHistoryEntry(tags.selected)
						continue
					}
					n.resetPreviewSelection()
					tags.dialog = &nativeTagDialog{action: action.id, tag: tags.selected, open: true, focus: true}
					if action.id == "rename" {
						tags.dialog.value = tags.selected
					}
				}
			}
		})
		if tags.failed {
			if uikit.Button(c, uikit.ButtonOptions{Label: n.text("common_retry", "Retry loading tags")}).Clicked() {
				n.loadTagCatalog()
			}
		} else if tags.loading && len(tags.names) == 0 {
			ui.Text(c, n.text("common_loading", "Loading tags…")).TextColor(c.Theme().TextMuted)
		} else if len(tags.names) == 0 {
			ui.Text(c, n.text("tag_library_empty", "No tags")).TextColor(c.Theme().TextMuted)
		}
	})
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
			if uikit.Button(c, uikit.ButtonOptions{Label: confirm, Disabled: disabled}).Clicked() {
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
			for _, existing := range tags.names {
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
			n.loadTagCatalog()
		})
	}()
}
