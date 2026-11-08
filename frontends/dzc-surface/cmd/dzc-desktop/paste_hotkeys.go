package main

import (
	"context"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

// Capture the foreground editor on the shortcut callback before scheduling work.
// Each action owns one connection; its target cannot be replaced by the WebView.
func prepareShortcutPaste(owner *client.Client, rich bool) (func() error, error) {
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	host, err := owner.DesktopConnection(ctx)
	if err != nil {
		cancel()
		return nil, err
	}
	state := host.Hello().State
	params := protocol.LibraryStateParams{ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch}
	var target protocol.QueueTarget
	if rich {
		var result protocol.OperationResult
		err = host.Call(ctx, protocol.MethodDesktopWindowTarget, struct{}{}, "OperationResult", &result)
		if err == nil && !result.Performed {
			err = client.ErrProtocol
		}
	} else {
		err = host.Call(ctx, protocol.MethodDesktopPasteTarget, params, "QueueTarget", &target)
	}
	if err != nil {
		host.Close()
		cancel()
		return nil, err
	}
	return func() error {
		defer cancel()
		defer host.Close()
		var queue protocol.QueueState
		if err := host.Call(ctx, protocol.MethodDesktopQueueGet, params, "QueueState", &queue); err != nil {
			return err
		}
		if !rich {
			if !queue.Preferences.Sequential || len(queue.Ids) == 0 {
				return nil
			}
			var result protocol.QueueStepResult
			return host.Call(ctx, protocol.MethodDesktopQueueNext, protocol.QueueNextParams{
				ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch,
				ExpectedRevision: queue.Revision, Target: target,
			}, "QueueStepResult", &result)
		}
		var snapshot protocol.BodySnapshot
		if err := host.Call(ctx, protocol.MethodDesktopHistoryPage, protocol.DesktopPageParams{Limit: 1}, "BodySnapshot", &snapshot); err != nil {
			return err
		}
		if snapshot.State.PrivacyEpoch != state.PrivacyEpoch || snapshot.State.ServiceInstanceId != state.ServiceInstanceId {
			return client.ErrProtocol
		}
		var entries protocol.DesktopEntries
		if err := host.ReadSnapshot(ctx, snapshot, "DesktopEntries", &entries); err != nil {
			return err
		}
		if len(entries.Entries) == 0 {
			return nil
		}
		withFormat := true
		return host.PasteWindowSelection(ctx, protocol.DesktopCopyParams{
			Id: entries.Entries[0].Id, ServiceInstanceId: state.ServiceInstanceId,
			ExpectedEpoch: state.PrivacyEpoch, Paste: true, WithFormat: &withFormat,
			DeleteAfterUse: &queue.Preferences.DeleteAfterPaste,
		})
	}, nil
}
