package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type nativeEntryEditor struct {
	ctx                              context.Context
	cancel                           context.CancelFunc
	id                               int64
	kind, content, tag, originalHash string
	message                          string
	open, loading, failed, focus     bool
	protected, confirmed             bool
}

func (n *nativeMain) canOpenHistoryDialog() bool {
	return n.session != nil && n.history.state.ServiceInstanceId != "" && !n.mutating && !n.failed && !n.hasHistoryDialog()
}

func (n *nativeMain) closeEntryEditor() {
	if form := n.editor; form != nil {
		form.cancel()
		form.content, form.originalHash = "", ""
		n.editor = nil
	}
}

func (n *nativeMain) addHistoryEntry(tag string) {
	if !n.canOpenHistoryDialog() || n.showSettings || n.showEmoji {
		return
	}
	n.resetPreviewSelection()
	ctx, cancel := context.WithCancel(n.session)
	n.editor = &nativeEntryEditor{ctx: ctx, cancel: cancel, kind: "text", tag: tag, open: true, focus: true}
}

func editableHistoryKind(kind string) bool {
	return kind == "text" || kind == "code"
}

func (n *nativeMain) editHistoryEntry(row protocol.DesktopEntry, generation uint64) {
	if !n.canOpenHistoryDialog() || generation != n.history.generation || n.showSettings ||
		!editableHistoryKind(row.ContentType) {
		return
	}
	protected := nativeConcealed(row)
	if protected && !n.canUseProtectedRow(row) {
		n.notice = n.text("privacy_reveal_first")
		return
	}
	n.resetPreviewSelection()
	session, state := n.session, n.history.state
	presentation, stop := context.WithCancel(session)
	form := &nativeEntryEditor{ctx: presentation, cancel: stop, id: row.Id, kind: row.ContentType, open: true,
		loading: true, protected: protected}
	n.editor = form
	go func() {
		ctx, cancel := context.WithTimeout(presentation, 5*time.Second)
		defer cancel()
		entry, err := n.library.Entry(ctx, protocol.DesktopEntryParams{
			Id: row.Id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, Reveal: protected,
		})
		n.update(session, func() {
			if n.editor != form {
				return
			}
			if protected && !mainWindowFocused(n.window) {
				n.closeEntryEditor()
				return
			}
			form.loading = false
			if err != nil || entry.Timestamp != row.Timestamp || entry.ContentType != row.ContentType {
				form.failed = true
				form.message = n.text("native_editor_load_failed")
				return
			}
			if !nativeEditorContentFits(entry.Content, true) {
				form.failed = true
				form.message = n.text("native_editor_too_large")
				return
			}
			form.content, form.focus = entry.Content, true
			form.originalHash = nativeEntryDigest(entry.ContentType, entry.Content)
		})
	}()
}

func (n *nativeMain) entryEditorView(c *ui.Context, kit uikit.Kit, form *nativeEntryEditor) {
	title := n.text("add_item")
	if form.id != 0 {
		title = n.text("edit_item")
	} else if form.tag != "" {
		title = n.text("add_item", "Add tagged entry")
	}
	kit.Dialog(c, &form.open, title, n.text("common_close"), n.mutating, func() {
		_, height := c.Size()
		ui.Scroll(c).FillWidth().MaxHeight(max(72, height-130)).Children(func() {
			ui.Column(c).FillWidth().Gap(7).Children(func() {
				if form.loading {
					ui.Text(c, n.text("common_loading"))
				} else if !form.failed {
					n.entryEditorFields(c, kit, form)
				}
				if form.message != "" {
					ui.Text(c, form.message).TextColor(c.Theme().Danger).Wrap()
				}
			})
		})
		ui.Row(c).Gap(6).Wrap().Justify(ui.End).Children(func() {
			if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("common_cancel"), Disabled: n.mutating}).Clicked() {
				form.open = false
			}
			if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("common_save"), Disabled: !n.canSaveEntry(form)}).Clicked() {
				n.saveHistoryEntry(form)
			}
		})
	})
	if !form.open {
		n.closeEntryEditor()
	}
}

func (n *nativeMain) entryEditorFields(c *ui.Context, kit uikit.Kit, form *nativeEntryEditor) {
	if form.id == 0 {
		kit.Select(c, &form.kind, n.text("native_entry_type"), []uikit.SelectOption{
			{Value: "text", Label: n.text("type_text")},
			{Value: "code", Label: n.text("type_code")},
		}, n.mutating)
	}
	field := kit.TextArea(c, &form.content, "").FillWidth().Height(126).
		Label(n.text("native_entry_text")).Placeholder(n.text("input_content_placeholder")).ReadOnly(n.mutating)
	if form.focus {
		field.Focus()
		form.focus = false
	}
	if n.private {
		label, hint := n.text("privacy_manual_confirm"), n.text("privacy_manual_hint")
		if form.id != 0 {
			label, hint = n.text("privacy_manual_edit_confirm"), n.text("privacy_manual_edit_hint")
		}
		kit.Checkbox(c, &form.confirmed, label).Disabled(n.mutating)
		ui.Text(c, hint).TextColor(c.Theme().TextMuted).Wrap()
	}
}

func (n *nativeMain) canSaveEntry(form *nativeEntryEditor) bool {
	return n.session != nil && !n.mutating && !form.failed && !form.loading &&
		strings.TrimSpace(form.content) != "" && (!n.private || form.confirmed)
}

func nativeEditorContentFits(content string, editing bool) bool {
	if len(content) > 32*1024*1024 {
		return false
	}
	if !editing {
		return true
	}
	if utf8.RuneCountInString(content) > 524288 {
		return false
	}
	encoded, _ := json.Marshal(content)
	return len(encoded) <= protocol.MaxFrameBytes-4096
}

func nativeEntryDigest(kind, content string) string {
	digest := sha256.New()
	digest.Write([]byte(kind))
	digest.Write([]byte{0})
	digest.Write([]byte(content))
	return hex.EncodeToString(digest.Sum(nil))
}

func (n *nativeMain) saveHistoryEntry(form *nativeEntryEditor) {
	if n.editor != form || !n.canSaveEntry(form) || form.protected && !mainWindowFocused(n.window) {
		return
	}
	if !nativeEditorContentFits(form.content, form.id != 0) {
		form.message = n.text("native_editor_too_large")
		return
	}
	n.mutating, form.message = true, ""
	session, state, lifetime := n.session, n.history.state, form.ctx
	id, kind, content, tag := form.id, form.kind, form.content, form.tag
	originalHash, confirmed, reveal := form.originalHash, n.private && form.confirmed, form.protected
	go func() {
		ctx, cancel := context.WithTimeout(lifetime, 6*time.Second)
		defer cancel()
		var err error
		if id == 0 {
			tags := []string{}
			if tag != "" {
				tags = append(tags, tag)
			}
			_, err = n.library.AddManual(ctx, content, protocol.ManualEntryParams{
				ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch,
				ContentType: kind, Tags: tags, ConfirmPersistence: confirmed,
			})
		} else {
			_, err = n.library.MutateEntry(ctx, protocol.DesktopMutationParams{
				Id: id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch,
				Action: "edit", Content: &content, ConfirmPersistence: &confirmed, Reveal: reveal,
				ExpectedEntrySha256: &originalHash,
			})
		}
		n.update(session, func() {
			n.mutating = false
			if n.editor != form {
				return
			}
			if err != nil {
				var remote *client.RemoteError
				if errors.As(err, &remote) && remote.Code == protocol.WireErrorCodeHistoryRevisionChanged {
					form.message = n.text("native_editor_conflict")
				} else {
					form.message = n.text("native_editor_save_failed")
				}
				return
			}
			n.closeEntryEditor()
		})
	}()
}
