package main

import (
	"context"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type nativeTagEditor struct {
	entry    int64
	original []string
	draft    []string
	name     string
	message  string
	open     bool
	focus    bool
}

func (n *nativeMain) editTags(row protocol.DesktopEntry, generation uint64) {
	if n.session == nil || n.failed || n.mutating || n.history.generation != generation {
		return
	}
	n.resetPreviewSelection()
	n.tags = &nativeTagEditor{
		entry: row.Id, original: slices.Clone(row.Tags), draft: slices.Clone(row.Tags), open: true, focus: true,
	}
}

func (e *nativeTagEditor) addTag() {
	name := strings.TrimSpace(e.name)
	if name != "" && !slices.Contains(e.draft, name) {
		e.draft = append(e.draft, name)
	}
	e.name = ""
}

func (n *nativeMain) tagEditorView(c *ui.Context, kit uikit.Kit, editor *nativeTagEditor) {
	kit.Dialog(c, &editor.open, n.text("native_edit_tags"), n.text("common_close"), n.mutating, func() {
		if len(editor.draft) > 0 {
			var removed string
			ui.Scroll(c).MaxHeight(105).Children(func() {
				ui.Column(c).Gap(4).Children(func() {
					for _, name := range editor.draft {
						ui.Row(c).FillWidth().AlignItems(ui.Center).Gap(6).Children(func() {
							ui.Text(c, name).Grow(1).SingleLine()
							remove := n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("native_remove"), Disabled: n.mutating})
							label := strings.ReplaceAll(n.text("native_remove_named_tag"), "{tag}", name)
							if remove.Label(label).Clicked() {
								removed = name
							}
						})
					}
				})
			})
			if removed != "" {
				editor.draft = slices.DeleteFunc(editor.draft, func(tag string) bool { return tag == removed })
			}
		}
		input := kit.TagNameField(c, &editor.name, n.text("tag_name"), n.mutating)
		input.OnTextKey(func(key ui.TextKey) bool {
			if key.Key != ui.KeyEnter || key.Mods != 0 || n.mutating {
				return false
			}
			editor.name = key.Text()
			editor.addTag()
			return true
		})
		if editor.focus {
			input.Focus()
			editor.focus = false
		}
		if editor.message != "" {
			ui.Text(c, editor.message).TextColor(c.Theme().Danger).Wrap()
		}
		ui.Row(c).Gap(6).Justify(ui.End).Children(func() {
			if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("native_add_tag"), Disabled: n.mutating || strings.TrimSpace(editor.name) == ""}).Clicked() {
				editor.addTag()
				editor.focus = true
			}
			if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("common_save"), Disabled: n.mutating}).Clicked() {
				n.saveTags(editor)
			}
		})
	})
	if !editor.open {
		n.tags = nil
	}
}

func (n *nativeMain) saveTags(editor *nativeTagEditor) {
	if n.session == nil || n.mutating || n.tags != editor || n.failed {
		return
	}
	editor.addTag()
	if slices.Equal(editor.original, editor.draft) {
		editor.open = false
		return
	}
	// The Host's metadata request accepts an empty array when all tags are removed.
	tags := append([]string{}, editor.draft...)
	session, state := n.session, n.history.state
	n.mutating, editor.message = true, ""
	go func() {
		ctx, cancel := context.WithTimeout(session, 4*time.Second)
		defer cancel()
		current, err := n.library.Summary(ctx, protocol.DesktopEntryParams{
			Id: editor.entry, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch,
		})
		conflict := err == nil && !slices.Equal(editor.original, current.Tags)
		if err == nil && !conflict {
			_, err = n.library.MutateEntry(ctx, protocol.DesktopMutationParams{
				Id: editor.entry, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch,
				Action: "tags", Tags: &tags,
			})
		}
		n.update(session, func() {
			n.mutating = false
			if conflict {
				editor.message = n.text("tag_selection_changed", "Tags changed. Close and reopen this editor to load the latest tags.")
				return
			}
			if err != nil {
				editor.message = n.text("tag_action_failed", "Could not save tags.")
				return
			}
			n.tags = nil
		})
	}()
}
