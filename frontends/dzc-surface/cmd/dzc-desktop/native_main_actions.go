package main

import (
	"context"
	"encoding/json"
	"errors"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

type nativeRowAction string

const (
	nativeCopy      nativeRowAction = "copy"
	nativeRichCopy  nativeRowAction = "rich-copy"
	nativePaste     nativeRowAction = "paste"
	nativeRichPaste nativeRowAction = "rich-paste"
	nativeBase64    nativeRowAction = "base64"
	nativeOpen      nativeRowAction = "open"
	nativeShare     nativeRowAction = "share"
	nativePin       nativeRowAction = "pin"
	nativeDelete    nativeRowAction = "delete"
)

func (n *nativeMain) hideWindow() {
	if n.window != nil {
		n.shell.hideMain(n.window)
	}
}

func (n *nativeMain) openClearHistory() {
	if n.session != nil && !n.mutating && !n.failed && n.history.state.ServiceInstanceId != "" {
		n.resetPreviewSelection()
		n.clearHistoryOpen = true
	}
}

func (n *nativeMain) clearHistory() {
	n.writeSetting(func(ctx context.Context, epoch uint64) error {
		return n.library.ClearHistory(ctx, epoch)
	})
}

func (n *nativeMain) rowAction(row protocol.DesktopEntry, generation uint64, action nativeRowAction) {
	if n.session == nil || n.history.generation != generation || n.mutating || n.failed {
		return
	}
	reveal := false
	if nativeConcealed(row) && action != nativePin && action != nativeDelete {
		if !n.canUseProtectedRow(row) {
			n.notice = n.text("privacy_reveal_first", "Reveal the sensitive entry first")
			return
		}
		reveal = true
	}
	n.mutating, n.notice = true, ""
	n.closePreview()
	session, state, preferences := n.session, n.history.state, n.preferences
	fromTagLibrary := n.tagLibrary != nil
	deleteAfterPaste := !fromTagLibrary && preferences["app.delete_after_paste"] == "true"

	timeout := 4 * time.Second
	switch action {
	case nativeOpen:
		timeout = nativeExternalApplicationTimeout
	case nativeShare:
		timeout = 16 * time.Second
	}
	go func() {
		ctx, cancel := context.WithTimeout(session, timeout)
		defer cancel()
		pasted, notice, err := n.runRowAction(ctx, row, state, preferences, action, reveal, deleteAfterPaste)
		n.update(session, func() {
			n.mutating = false
			if err != nil {
				if action == nativeShare {
					n.notice = n.text("native_share_unavailable", "Sharing is unavailable")
				} else {
					n.notice = n.text("privacy_action_failed", "Clipboard action failed")
				}
				return
			}
			n.notice = notice
			if pasted {
				if !fromTagLibrary {
					n.query = ""
				}
				if !n.pinned {
					n.shell.hideMain(n.window)
				}
			}
		})
	}()
}

func (n *nativeMain) dropFiles(paths []string) {
	state := n.history.state
	if n.session == nil || n.failed || state.ServiceInstanceId == "" || len(paths) == 0 || len(paths) > 256 {
		return
	}
	unique := make(map[string]struct{}, len(paths))
	files := make([]string, 0, len(paths))
	for _, path := range paths {
		if _, ok := unique[path]; !ok {
			unique[path] = struct{}{}
			files = append(files, path)
		}
	}
	params := protocol.FileDropParams{
		Paths: files, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch,
	}
	session := n.session
	go func() {
		ctx, cancel := context.WithTimeout(session, 4*time.Second)
		defer cancel()
		if err := n.library.dropFiles(ctx, params); err != nil {
			n.update(session, func() { n.notice = n.text("native_drop_failed") })
		}
	}()
}

func (n *nativeMain) runRowAction(ctx context.Context, row protocol.DesktopEntry, state protocol.RuntimeState, preferences map[string]string, action nativeRowAction, reveal, deleteAfterPaste bool) (bool, string, error) {
	switch action {
	case nativeShare:
		_, err := n.library.ShareEntry(ctx, protocol.ShareEntryParams{
			Id: row.Id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, Reveal: reveal,
		})
		return false, "", err
	case nativeOpen:
		err := n.library.OpenEntry(ctx, protocol.ExternalOpenParams{
			Id: row.Id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, Reveal: reveal,
		})
		return false, "", err
	case nativePin, nativeDelete:
		params := protocol.DesktopMutationParams{
			Id: row.Id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, Action: string(action),
		}
		if action == nativePin {
			pinned := !row.IsPinned
			params.Pinned = &pinned
		}
		_, err := n.library.MutateEntry(ctx, params)
		return false, "", err
	default:
		return n.transferRow(ctx, row.Id, state, preferences, action, reveal, deleteAfterPaste)
	}
}

func (n *nativeMain) transferRow(ctx context.Context, id int64, state protocol.RuntimeState, preferences map[string]string, action nativeRowAction, reveal, deleteAfterPaste bool) (bool, string, error) {
	params := protocol.DesktopCopyParams{Id: id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, Reveal: reveal}
	if action == nativePaste || action == nativeRichPaste {
		paste, err := n.canAutoPaste(ctx)
		if err != nil {
			return false, "", err
		}
		params.Paste = paste
	}
	if action == nativeRichPaste || action == nativeRichCopy {
		withFormat := true
		params.WithFormat = &withFormat
	}
	if action == nativeBase64 {
		base64 := true
		params.ImageAsBase64 = &base64
	}
	if params.Paste {
		params.DeleteAfterUse = &deleteAfterPaste
	}
	outcome, err := n.actions.CopyEntry(ctx, params)
	if err != nil {
		return false, "", err
	}
	if outcome == "target_required" && params.Paste {
		// The Host guarantees that this outcome preceded dispatch. Retry only as a copy.
		params.Paste, params.DeleteAfterUse = false, nil
		outcome, err = n.actions.CopyEntry(ctx, params)
		if err != nil {
			return false, "", err
		}
	}
	if outcome != "completed" {
		return false, "", errors.New("unexpected clipboard action outcome")
	}
	if params.Paste {
		return true, "", nil
	}
	return false, mainLanguage(preferences["app.language"]).text("screenshot_captured"), nil
}

func (n *nativeMain) canAutoPaste(ctx context.Context) (bool, error) {
	raw, err := n.library.Platform(ctx)
	if err != nil {
		return false, err
	}
	type capability struct {
		Availability struct{ Status string }
	}
	var platform struct {
		NativeWayland bool `json:"native_wayland"`
		Write         capability
		AutoPaste     capability `json:"auto_paste"`
	}
	if json.Unmarshal(raw, &platform) != nil || platform.Write.Availability.Status != "available" {
		return false, errors.New("clipboard write unavailable")
	}
	if platform.AutoPaste.Availability.Status == "" {
		return false, errors.New("automatic paste capability unavailable")
	}
	return !platform.NativeWayland && platform.AutoPaste.Availability.Status == "available", nil
}
