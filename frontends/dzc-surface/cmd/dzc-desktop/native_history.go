package main

import (
	"context"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

type nativeHistoryFilter struct {
	query, kind, tag, sort string
}

type nativeHistoryRequest struct {
	generation uint64
	state      protocol.RuntimeState
	filter     nativeHistoryFilter
	offset     uint32
}

type nativeHistoryPage struct {
	state protocol.RuntimeState
	rows  []protocol.DesktopEntry
	more  bool
}

func (l *Library) nativeHistoryPage(ctx context.Context, request nativeHistoryRequest) (nativeHistoryPage, error) {
	if request.filter.tag != "" {
		return l.nativeTagPage(ctx, request)
	}
	params := protocol.DesktopPageParams{Offset: request.offset, Limit: pageSize + 1}
	if request.filter.query != "" {
		params.Query = &request.filter.query
	}
	if request.filter.kind != "" {
		params.Kind = &request.filter.kind
	}
	var result nativeHistoryPage
	err := readLibrarySnapshot(ctx, func(ctx context.Context) error {
		var page protocol.DesktopEntries
		state, err := l.readSnapshotState(ctx, protocol.MethodDesktopHistoryPage, params, "DesktopEntries", &page, request.state.PrivacyEpoch)
		if err == nil {
			result = nativeHistoryPage{
				state: state, rows: page.Entries[:min(pageSize, len(page.Entries))], more: len(page.Entries) > pageSize,
			}
		}
		return err
	})
	return result, err
}

func (l *Library) nativeTagPage(ctx context.Context, request nativeHistoryRequest) (nativeHistoryPage, error) {
	params := protocol.DesktopTagPageParams{
		Tag: request.filter.tag, Query: request.filter.query, SortBy: request.filter.sort, Limit: pageSize,
		Offset: request.offset, ServiceInstanceId: request.state.ServiceInstanceId, ExpectedEpoch: request.state.PrivacyEpoch,
	}
	var result nativeHistoryPage
	err := readLibrarySnapshot(ctx, func(ctx context.Context) error {
		var page protocol.DesktopTagItemsPage
		state, err := l.readSnapshotState(ctx, protocol.MethodDesktopTagsPage, params, "DesktopTagItemsPage", &page, request.state.PrivacyEpoch)
		if err != nil {
			return err
		}
		if page.HasMore && (len(page.Items) == 0 || page.NextOffset == nil ||
			*page.NextOffset != request.offset+uint32(len(page.Items))) {
			return client.ErrProtocol
		}
		result = nativeHistoryPage{state: state, rows: page.Items, more: page.HasMore}
		return nil
	})
	return result, err
}

// nativeHistory owns the rows for one live main-window presentation. A
// generation change retires pending reads, including a search that comes
// back to the same query after the user has already dismissed it.
type nativeHistory struct {
	state      protocol.RuntimeState
	generation uint64
	filter     nativeHistoryFilter
	rows       []protocol.DesktopEntry
	selected   int
	more       bool
}

func (h *nativeHistory) clear() {
	h.generation++
	clear(h.rows)
	h.rows = nil
	h.selected = -1
	h.more = false
}

func (h *nativeHistory) reset(state protocol.RuntimeState, filter nativeHistoryFilter) {
	h.clear()
	h.state, h.filter = state, filter
}

func (h *nativeHistory) request() nativeHistoryRequest {
	return nativeHistoryRequest{
		generation: h.generation, state: h.state, filter: h.filter, offset: uint32(len(h.rows)),
	}
}

func (h *nativeHistory) accepts(request nativeHistoryRequest) bool {
	return request.generation == h.generation && request.offset == uint32(len(h.rows))
}

func (h *nativeHistory) apply(request nativeHistoryRequest, page nativeHistoryPage) bool {
	if !h.accepts(request) || page.state.ServiceInstanceId != h.state.ServiceInstanceId ||
		page.state.PrivacyEpoch != h.state.PrivacyEpoch || page.state.HistoryRevision < h.state.HistoryRevision {
		return false
	}
	// Appending a page from a newer revision can skip or duplicate rows
	// because the service's offsets have moved.
	if request.offset > 0 && page.state.HistoryRevision != h.state.HistoryRevision {
		return false
	}
	page.state.EventSequence = max(page.state.EventSequence, h.state.EventSequence)
	h.state, h.more = page.state, page.more
	h.rows = append(h.rows, page.rows...)
	return true
}
