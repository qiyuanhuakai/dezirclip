package application

import (
	"context"
	"errors"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func (r LibraryReader) HistoryPage(ctx context.Context, params protocol.DesktopPageParams, expected protocol.RuntimeState) ([]protocol.DesktopEntry, protocol.RuntimeState, error) {
	if expected.ServiceInstanceId != r.owner.Hello().State.ServiceInstanceId {
		return nil, protocol.RuntimeState{}, errors.New("ClipboardHostInstanceChanged")
	}
	var page protocol.DesktopEntries
	state, err := r.ReadSnapshot(ctx, protocol.MethodDesktopHistoryPage, params, "DesktopEntries", &page, expected.PrivacyEpoch)
	if err != nil {
		return nil, protocol.RuntimeState{}, err
	}
	return page.Entries, state, nil
}

func (r LibraryReader) TagPage(ctx context.Context, params protocol.DesktopTagPageParams) (protocol.DesktopTagItemsPage, protocol.RuntimeState, error) {
	if params.ServiceInstanceId != r.owner.Hello().State.ServiceInstanceId {
		return protocol.DesktopTagItemsPage{}, protocol.RuntimeState{}, errors.New("ClipboardHostInstanceChanged")
	}
	var page protocol.DesktopTagItemsPage
	state, err := r.ReadSnapshot(ctx, protocol.MethodDesktopTagsPage, params, "DesktopTagItemsPage", &page, params.ExpectedEpoch)
	if err != nil {
		return protocol.DesktopTagItemsPage{}, protocol.RuntimeState{}, err
	}
	if page.HasMore && (len(page.Items) == 0 || page.NextOffset == nil || uint64(*page.NextOffset) != uint64(params.Offset)+uint64(len(page.Items))) {
		return protocol.DesktopTagItemsPage{}, protocol.RuntimeState{}, client.ErrProtocol
	}
	return page, state, nil
}

func (r LibraryReader) Entry(ctx context.Context, params protocol.DesktopEntryParams) (protocol.DesktopEntry, protocol.RuntimeState, error) {
	return r.readEntry(ctx, protocol.MethodDesktopHistoryGet, params)
}

func (r LibraryReader) Summary(ctx context.Context, params protocol.DesktopEntryParams) (protocol.DesktopEntry, protocol.RuntimeState, error) {
	params.Reveal = false
	params.ThumbnailDimension = nil
	return r.readEntry(ctx, protocol.MethodDesktopHistorySummary, params)
}

func (r LibraryReader) readEntry(ctx context.Context, method protocol.Method, params protocol.DesktopEntryParams) (protocol.DesktopEntry, protocol.RuntimeState, error) {
	instance := r.owner.Hello().State.ServiceInstanceId
	if (params.Id < 0 || params.ServiceInstanceId != "") && params.ServiceInstanceId != instance {
		return protocol.DesktopEntry{}, protocol.RuntimeState{}, errors.New("ClipboardHostInstanceChanged")
	}
	params.ServiceInstanceId = instance
	var entry protocol.DesktopEntry
	state, err := r.ReadSnapshot(ctx, method, params, "DesktopEntry", &entry, params.ExpectedEpoch)
	if err != nil {
		return protocol.DesktopEntry{}, protocol.RuntimeState{}, err
	}
	if entry.Id != params.Id {
		return protocol.DesktopEntry{}, protocol.RuntimeState{}, client.ErrProtocol
	}
	return entry, state, nil
}
