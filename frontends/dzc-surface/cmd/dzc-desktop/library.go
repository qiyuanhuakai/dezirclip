package main

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/desktop/application"
)

// Library adapts authenticated Host results to desktop callers and the Web bridge contract.
type Library struct {
	owner *client.Client
}

type hostedEntry struct {
	protocol.DesktopEntry
	Instance string `json:"host_instance_id"`
}

func (l *Library) Status(ctx context.Context) (protocol.ServiceStatus, error) {
	return l.readStatus(ctx, nil)
}

func (l *Library) readStatus(ctx context.Context, observe func(error)) (protocol.ServiceStatus, error) {
	var status protocol.ServiceStatus
	err := readPrivacyStatus(ctx, func() error {
		status = protocol.ServiceStatus{}
		err := l.owner.DesktopCall(ctx, protocol.MethodServiceStatus, struct{}{}, "ServiceStatus", &status)
		if observe != nil {
			observe(err)
		}
		return err
	})
	return status, err
}

func (l *Library) Platform(ctx context.Context) (json.RawMessage, error) {
	var result protocol.DesktopPlatformReport
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopPlatformProbe, struct{}{}, "DesktopPlatformReport", &result)
	return result.Report, err
}

func (l *Library) SetPrivacy(ctx context.Context, params protocol.PrivacySetParams) (protocol.ServiceStatus, error) {
	if !params.Active && params.DurationMs != nil {
		return protocol.ServiceStatus{}, errors.New("PrivacyDurationInvalid")
	}
	var privacy protocol.PrivacyState
	if err := l.owner.DesktopCall(ctx, protocol.MethodPrivacySet, params, "PrivacyState", &privacy); err != nil {
		return protocol.ServiceStatus{}, err
	}
	status, err := l.Status(ctx)
	if err == nil && status.Privacy.PrivacyEpoch != privacy.PrivacyEpoch {
		return protocol.ServiceStatus{}, errors.New("PrivacyEpochChanged")
	}
	return status, err
}

func (l *Library) SetPrivacyHotkey(ctx context.Context, hotkey string) (string, error) {
	var result protocol.PrivacyHotkey
	err := l.owner.DesktopCall(ctx, protocol.MethodPrivacyHotkeySet, protocol.PrivacyHotkey{Hotkey: hotkey}, "PrivacyHotkey", &result)
	var remote *client.RemoteError
	if errors.As(err, &remote) {
		switch remote.Code {
		case protocol.WireErrorCodeBusy:
			return "", errors.New("PrivacyHotkeyBusy")
		case protocol.WireErrorCodeUnsupportedCapability:
			return "", errors.New("PrivacyHotkeyUnavailable")
		}
	}
	return result.Hotkey, err
}

func (l *Library) reader() application.LibraryReader {
	if desktopTimingEnabled() {
		return application.NewLibraryReader(l.owner, desktopSnapshotTiming)
	}
	return application.NewLibraryReader(l.owner, nil)
}

func (l *Library) snapshot(ctx context.Context, method protocol.Method, params any, schema string, result any, epoch uint64) error {
	_, err := l.reader().ReadSnapshot(ctx, method, params, schema, result, epoch)
	return err
}

func (l *Library) readSnapshot(ctx context.Context, method protocol.Method, params any, schema string, result any, epoch uint64) error {
	_, err := l.reader().ReadSnapshotOnce(ctx, method, params, schema, result, epoch)
	return err
}

func (l *Library) Page(ctx context.Context, params protocol.DesktopPageParams, epoch uint64) ([]hostedEntry, error) {
	expected := l.owner.Hello().State
	expected.PrivacyEpoch = epoch
	entries, state, err := l.reader().HistoryPage(ctx, params, expected)
	if err != nil {
		return nil, err
	}
	return hostedRows(entries, state.ServiceInstanceId), nil
}

func hostedRows(entries []protocol.DesktopEntry, instance string) []hostedEntry {
	rows := make([]hostedEntry, len(entries))
	for i, entry := range entries {
		rows[i] = hostedEntry{entry, instance}
	}
	return rows
}

type tagPage struct {
	Items      []hostedEntry `json:"items"`
	Total      uint64        `json:"total"`
	HasMore    bool          `json:"has_more"`
	NextOffset *uint32       `json:"next_offset"`
}

func (l *Library) TagPage(ctx context.Context, params protocol.DesktopTagPageParams) (tagPage, error) {
	params.ServiceInstanceId = l.owner.Hello().State.ServiceInstanceId
	page, state, err := l.reader().TagPage(ctx, params)
	if err != nil {
		return tagPage{}, err
	}
	return tagPage{hostedRows(page.Items, state.ServiceInstanceId), page.Total, page.HasMore, page.NextOffset}, nil
}

func (l *Library) Entry(ctx context.Context, params protocol.DesktopEntryParams) (hostedEntry, error) {
	entry, state, err := l.reader().Entry(ctx, params)
	return hostedEntry{entry, state.ServiceInstanceId}, err
}

func (l *Library) OpenEntry(ctx context.Context, params protocol.ExternalOpenParams) error {
	if params.ServiceInstanceId != l.owner.Hello().State.ServiceInstanceId || params.Id <= 0 {
		return client.ErrProtocol
	}
	var result protocol.OperationResult
	if err := l.owner.DesktopCall(ctx, protocol.MethodDesktopHistoryOpen, params, "OperationResult", &result); err != nil {
		return err
	}
	if !result.Performed {
		return client.ErrProtocol
	}
	return nil
}

func (l *Library) ShareEntry(ctx context.Context, params protocol.ShareEntryParams) (bool, error) {
	if params.ServiceInstanceId != l.owner.Hello().State.ServiceInstanceId || params.Id == 0 {
		return false, client.ErrProtocol
	}
	var result protocol.OperationResult
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopHistoryShare, params, "OperationResult", &result)
	return result.Performed, err
}

func (l *Library) Summary(ctx context.Context, params protocol.DesktopEntryParams) (hostedEntry, error) {
	entry, state, err := l.reader().Summary(ctx, params)
	return hostedEntry{entry, state.ServiceInstanceId}, err
}

func (l *Library) TagCounts(ctx context.Context, epoch uint64) (json.RawMessage, error) {
	params := protocol.LibraryStateParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch}
	var counts protocol.LibraryCounts
	if err := l.snapshot(ctx, protocol.MethodDesktopTagsCounts, params, "LibraryCounts", &counts, epoch); err != nil {
		return nil, err
	}
	return counts.Values, nil
}

func (l *Library) MutateEntry(ctx context.Context, params protocol.DesktopMutationParams) (int64, error) {
	return application.MutateEntry(ctx, l.owner, params)
}

func (l *Library) AddManual(ctx context.Context, content string, params protocol.ManualEntryParams) (int64, error) {
	return application.AddManual(ctx, l.owner, content, params)
}

func (l *Library) TagColors(ctx context.Context, epoch uint64) (json.RawMessage, error) {
	params := protocol.LibraryStateParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch}
	var colors protocol.LibraryColors
	err := l.snapshot(ctx, protocol.MethodDesktopTagsColors, params, "LibraryColors", &colors, epoch)
	return colors.Values, err
}

func (l *Library) EditTag(ctx context.Context, action string, name string, newName string, epoch uint64) error {
	instance := l.owner.Hello().State.ServiceInstanceId
	var method protocol.Method
	var params any = protocol.LibraryTagParams{ServiceInstanceId: instance, ExpectedEpoch: epoch, Name: name}
	switch action {
	case "create":
		method = protocol.MethodDesktopTagsCreate
	case "remove":
		method = protocol.MethodDesktopTagsRemove
	case "rename":
		method = protocol.MethodDesktopTagsRename
		params = protocol.LibraryRenameParams{ServiceInstanceId: instance, ExpectedEpoch: epoch, Name: name, NewName: newName}
	default:
		return client.ErrProtocol
	}
	return l.edit(ctx, method, params)
}

func (l *Library) edit(ctx context.Context, method protocol.Method, params any) error {
	var result protocol.OperationResult
	err := l.owner.DesktopCall(ctx, method, params, "OperationResult", &result)
	if err == nil && !result.Performed {
		return client.ErrProtocol
	}
	return err
}

func (l *Library) ClearHistory(ctx context.Context, epoch uint64) error {
	return l.edit(ctx, protocol.MethodDesktopHistoryClear, protocol.LibraryStateParams{
		ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch,
	})
}

func (l *Library) SetWindowPinned(ctx context.Context, pinned bool, epoch uint64) error {
	return l.edit(ctx, protocol.MethodDesktopWindowPin, protocol.WindowPinParams{
		ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Pinned: pinned,
	})
}

func (l *Library) ReorderPins(ctx context.Context, pairs [][2]int64, epoch uint64) error {
	orders := make([]protocol.LibraryPinOrder, len(pairs))
	for i, pair := range pairs {
		orders[i] = protocol.LibraryPinOrder{Id: pair[0], Order: pair[1]}
	}
	return l.edit(ctx, protocol.MethodDesktopHistoryReorder, protocol.LibraryOrderParams{
		ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Orders: orders,
	})
}

func (l *Library) movePin(ctx context.Context, state protocol.RuntimeState, move protocol.LibraryPinMove) error {
	return l.edit(ctx, protocol.MethodDesktopHistoryReorder, protocol.LibraryOrderParams{
		ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch,
		Orders: []protocol.LibraryPinOrder{}, RelativeOrder: &move,
	})
}

func (l *Library) DeleteTagEntries(ctx context.Context, params protocol.LibraryTagBatchParams) (uint64, error) {
	if params.ServiceInstanceId == "" {
		params.ServiceInstanceId = l.owner.Hello().State.ServiceInstanceId
	}
	return l.deleteEntries(ctx, protocol.MethodDesktopTagsDeleteEntries, params)
}

func (l *Library) DeleteTagWithContents(ctx context.Context, name string, epoch uint64) (uint64, error) {
	return l.deleteEntries(ctx, protocol.MethodDesktopTagsDeleteContents, protocol.LibraryTagParams{
		ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Name: name,
	})
}

func (l *Library) deleteEntries(ctx context.Context, method protocol.Method, params any) (uint64, error) {
	var result protocol.LibraryDeleted
	err := l.owner.DesktopCall(ctx, method, params, "LibraryDeleted", &result)
	if err == nil && result.CleanupFailed {
		err = errors.New("History was deleted, but attachment cleanup failed")
	}
	return result.DeletedCount, err
}

func (l *Library) Preferences(ctx context.Context) (json.RawMessage, error) {
	status, err := l.Status(ctx)
	if err != nil {
		return nil, err
	}
	return l.preferences(ctx, status.State)
}

func (l *Library) preferences(ctx context.Context, state protocol.RuntimeState) (json.RawMessage, error) {
	params := protocol.LibraryStateParams{ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch}
	var preferences protocol.DesktopPreferences
	err := l.snapshot(ctx, protocol.MethodDesktopSettingsPreferences, params, "DesktopPreferences", &preferences, params.ExpectedEpoch)
	return preferences.Values, err
}

func (l *Library) PasteQueue(ctx context.Context, epoch uint64) ([]int64, error) {
	var result protocol.QueueState
	params := protocol.LibraryStateParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch}
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopQueueGet, params, "QueueState", &result)
	return result.Ids, err
}

func (l *Library) queueStatus(ctx context.Context, state protocol.RuntimeState) (protocol.QueueStatus, error) {
	params := protocol.LibraryStateParams{ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch}
	return readQueueStatus(ctx, l.owner.DesktopCall, params)
}

func (l *Library) SetPasteQueue(ctx context.Context, epoch uint64, ids []int64) error {
	for _, id := range ids {
		if id <= 0 {
			return errors.New("SessionQueueUnavailable")
		}
	}
	return l.edit(ctx, protocol.MethodDesktopQueueSet, protocol.QueueReplaceParams{
		ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Ids: ids,
	})
}

func (l *Library) SetSetting(ctx context.Context, key, value string, epoch uint64) error {
	return application.SetSetting(ctx, l.owner, key, value, epoch)
}

func (l *Library) SourceIcon(ctx context.Context, params protocol.DesktopEntryParams) (*string, error) {
	var icon protocol.SourceAppIcon
	err := l.snapshot(ctx, protocol.MethodDesktopHistorySourceIcon, params, "SourceAppIcon", &icon, params.ExpectedEpoch)
	return icon.Src, err
}
