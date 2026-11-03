package main

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

// Library is the shared React window's authenticated view of the Rust Host.
type Library struct {
	owner *client.Client
}

type hostedEntry struct {
	protocol.DesktopEntry
	Instance string `json:"host_instance_id"`
}

func (l *Library) Status(ctx context.Context) (protocol.ServiceStatus, error) {
	var status protocol.ServiceStatus
	err := l.owner.DesktopCall(ctx, protocol.MethodServiceStatus, struct{}{}, "ServiceStatus", &status)
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

// The descriptor and its chunks must be read on the same authenticated stream.
func (l *Library) snapshot(ctx context.Context, method protocol.Method, params any, schema string, result any, epoch uint64) error {
	host, err := l.owner.DesktopConnection(ctx)
	if err != nil {
		return err
	}
	defer host.Close()
	var body protocol.BodySnapshot
	if err := host.Call(ctx, method, params, "BodySnapshot", &body); err != nil {
		return err
	}
	if body.State.PrivacyEpoch != epoch {
		return errors.New("PrivacyEpochChanged")
	}
	return host.ReadSnapshot(ctx, body, schema, result)
}

func (l *Library) Page(ctx context.Context, params protocol.DesktopPageParams, epoch uint64) ([]hostedEntry, error) {
	var page protocol.DesktopEntries
	if err := l.snapshot(ctx, protocol.MethodDesktopHistoryPage, params, "DesktopEntries", &page, epoch); err != nil {
		return nil, err
	}
	return l.rows(page.Entries), nil
}

func (l *Library) rows(entries []protocol.DesktopEntry) []hostedEntry {
	rows := make([]hostedEntry, len(entries))
	for i, entry := range entries {
		rows[i] = hostedEntry{entry, l.owner.Hello().State.ServiceInstanceId}
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
	var page protocol.DesktopTagItemsPage
	if err := l.snapshot(ctx, protocol.MethodDesktopTagsPage, params, "DesktopTagItemsPage", &page, params.ExpectedEpoch); err != nil {
		return tagPage{}, err
	}
	return tagPage{l.rows(page.Items), page.Total, page.HasMore, page.NextOffset}, nil
}

func (l *Library) Entry(ctx context.Context, params protocol.DesktopEntryParams) (hostedEntry, error) {
	return l.entry(ctx, protocol.MethodDesktopHistoryGet, params)
}

func (l *Library) Summary(ctx context.Context, params protocol.DesktopEntryParams) (hostedEntry, error) {
	params.Reveal = false
	params.ThumbnailDimension = nil
	return l.entry(ctx, protocol.MethodDesktopHistorySummary, params)
}

func (l *Library) TagCounts(ctx context.Context, epoch uint64) (json.RawMessage, error) {
	params := protocol.LibraryStateParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch}
	var counts protocol.LibraryCounts
	if err := l.snapshot(ctx, protocol.MethodDesktopTagsCounts, params, "LibraryCounts", &counts, epoch); err != nil {
		return nil, err
	}
	return counts.Values, nil
}

// MutateEntry uses the identity and privacy epoch of the row the user acted on.
func (l *Library) MutateEntry(ctx context.Context, params protocol.DesktopMutationParams) (int64, error) {
	if params.ServiceInstanceId != l.owner.Hello().State.ServiceInstanceId || params.Id == 0 {
		return 0, client.ErrProtocol
	}
	switch params.Action {
	case "pin", "tags", "edit", "delete":
	default:
		return 0, client.ErrProtocol
	}
	var result protocol.DesktopMutationResult
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopHistoryMutate, params, "DesktopMutationResult", &result)
	if err == nil && (!result.Performed || result.Id == 0) {
		return 0, client.ErrProtocol
	}
	return result.Id, err
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

func (l *Library) ReorderPins(ctx context.Context, pairs [][2]int64, epoch uint64) error {
	orders := make([]protocol.LibraryPinOrder, len(pairs))
	for i, pair := range pairs {
		orders[i] = protocol.LibraryPinOrder{Id: pair[0], Order: pair[1]}
	}
	return l.edit(ctx, protocol.MethodDesktopHistoryReorder, protocol.LibraryOrderParams{
		ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Orders: orders,
	})
}

func (l *Library) DeleteTagEntries(ctx context.Context, params protocol.LibraryTagBatchParams) (uint64, error) {
	params.ServiceInstanceId = l.owner.Hello().State.ServiceInstanceId
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
	params := protocol.LibraryStateParams{ServiceInstanceId: status.State.ServiceInstanceId, ExpectedEpoch: status.State.PrivacyEpoch}
	var preferences protocol.DesktopPreferences
	if err := l.snapshot(ctx, protocol.MethodDesktopSettingsPreferences, params, "DesktopPreferences", &preferences, params.ExpectedEpoch); err != nil {
		return nil, err
	}
	return preferences.Values, nil
}

func (l *Library) entry(ctx context.Context, method protocol.Method, params protocol.DesktopEntryParams) (hostedEntry, error) {
	instance := l.owner.Hello().State.ServiceInstanceId
	if (params.Id < 0 || params.ServiceInstanceId != "") && params.ServiceInstanceId != instance {
		return hostedEntry{}, errors.New("ClipboardHostInstanceChanged")
	}
	params.ServiceInstanceId = instance
	var entry protocol.DesktopEntry
	if err := l.snapshot(ctx, method, params, "DesktopEntry", &entry, params.ExpectedEpoch); err != nil {
		return hostedEntry{}, err
	}
	if entry.Id != params.Id {
		return hostedEntry{}, client.ErrProtocol
	}
	return hostedEntry{entry, instance}, nil
}
