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
	nativePaste     nativeRowAction = "paste"
	nativeRichPaste nativeRowAction = "rich-paste"
	nativeBase64    nativeRowAction = "base64"
	nativeOpen      nativeRowAction = "open"
	nativePin       nativeRowAction = "pin"
	nativeDelete    nativeRowAction = "delete"
)

func (n *nativeMain) rowAction(row protocol.DesktopEntry, generation uint64, action nativeRowAction) {
	if n.session == nil || n.history.generation != generation || n.mutating || n.failed {
		return
	}
	n.mutating, n.notice = true, ""
	n.closePreview()
	session, state, preferences := n.session, n.history.state, n.preferences
	go func() {
		ctx, cancel := context.WithTimeout(session, 4*time.Second)
		defer cancel()
		pasted, notice, err := n.runRowAction(ctx, row, state, preferences, action)
		n.update(session, func() {
			n.mutating = false
			if err != nil {
				n.notice = "Clipboard action failed"
				return
			}
			n.notice = notice
			if pasted {
				n.query = ""
				if !n.pinned {
					n.shell.hideMain(n.window)
				}
			}
		})
	}()
}

func (n *nativeMain) runRowAction(ctx context.Context, row protocol.DesktopEntry, state protocol.RuntimeState, preferences map[string]string, action nativeRowAction) (bool, string, error) {
	switch action {
	case nativeOpen:
		err := n.library.OpenEntry(ctx, protocol.ExternalOpenParams{
			Id: row.Id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch,
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
		return n.transferRow(ctx, row.Id, state, preferences, action)
	}
}

func (n *nativeMain) transferRow(ctx context.Context, id int64, state protocol.RuntimeState, preferences map[string]string, action nativeRowAction) (bool, string, error) {
	params := protocol.DesktopCopyParams{Id: id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch}
	if action == nativePaste || action == nativeRichPaste {
		raw, err := n.library.Platform(ctx)
		if err != nil {
			return false, "", err
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
			return false, "", errors.New("clipboard write unavailable")
		}
		if platform.AutoPaste.Availability.Status == "" {
			return false, "", errors.New("automatic paste capability unavailable")
		}
		params.Paste = !platform.NativeWayland && platform.AutoPaste.Availability.Status == "available"
	}
	if action == nativeRichPaste {
		withFormat := true
		params.WithFormat = &withFormat
	}
	if action == nativeBase64 {
		base64 := true
		params.ImageAsBase64 = &base64
	}
	if params.Paste {
		deleteAfterUse := preferences["app.delete_after_paste"] == "true"
		moveToTop := preferences["app.move_to_top_after_paste"] != "false"
		params.DeleteAfterUse, params.MoveToTop = &deleteAfterUse, &moveToTop
	}
	outcome, err := n.actions.CopyEntry(ctx, params)
	if err != nil {
		return false, "", err
	}
	if outcome == "target_required" && params.Paste {
		// The Host guarantees that this outcome preceded dispatch. Retry only as a copy.
		params.Paste, params.DeleteAfterUse, params.MoveToTop = false, nil, nil
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
	return false, "Copied to clipboard", nil
}
