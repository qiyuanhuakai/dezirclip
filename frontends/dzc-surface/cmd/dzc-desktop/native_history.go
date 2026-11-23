package main

import (
	"context"
	"slices"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

type nativeHistoryFilter struct {
	query, kind, tag, sort string
}

type nativeHistoryRequest struct {
	generation   uint64
	state        protocol.RuntimeState
	filter       nativeHistoryFilter
	offset       uint32
	refreshLimit int
}

type nativeHistoryPage struct {
	state protocol.RuntimeState
	rows  []protocol.DesktopEntry
	more  bool
	total uint64
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
	if request.filter.kind != "" {
		params.Kind = &request.filter.kind
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
		result = nativeHistoryPage{state: state, rows: page.Items, more: page.HasMore, total: page.Total}
		return nil
	})
	return result, err
}

// nativeHistory owns the rows for one live main-window presentation. A
// generation change retires pending reads, including a search that comes
// back to the same query after the user has already dismissed it.
type nativeHistory struct {
	state        protocol.RuntimeState
	generation   uint64
	filter       nativeHistoryFilter
	rows         []protocol.DesktopEntry
	selected     int
	more         bool
	total        uint64
	refreshLimit int
}

func (h *nativeHistory) clear() {
	h.generation++
	clear(h.rows)
	h.rows = nil
	h.selected = -1
	h.more = false
	h.total = 0
	h.refreshLimit = 0
}

func (h *nativeHistory) reset(state protocol.RuntimeState, filter nativeHistoryFilter) {
	h.clear()
	h.state, h.filter = state, filter
}

// Keep the previous range visible only when the event stream accounts for
// the new revision. A reset or a privacy change must retire the whole range.
func historyRefreshEntries(current protocol.RuntimeState, result protocol.EventsResult) (map[int64]struct{}, bool) {
	next := result.State
	if result.ResyncRequired || next.ServiceInstanceId != current.ServiceInstanceId || next.PrivacyEpoch != current.PrivacyEpoch {
		return nil, false
	}
	affected := make(map[int64]struct{})
	coveredRevision := current.HistoryRevision
	for _, event := range result.Events {
		switch event.Event {
		case protocol.EventKindHistoryReset, protocol.EventKindPrivacyStateChanged:
			return nil, false
		case protocol.EventKindEntryAdded, protocol.EventKindEntryDeleted, protocol.EventKindEntryUpdated:
			if event.State.HistoryRevision <= current.HistoryRevision {
				continue
			}
			if event.Data.EntryId == nil || *event.Data.EntryId == 0 || event.State.HistoryRevision > coveredRevision+1 ||
				event.State.PrivacyEpoch != current.PrivacyEpoch ||
				event.State.ServiceInstanceId != current.ServiceInstanceId {
				return nil, false
			}
			coveredRevision = max(coveredRevision, event.State.HistoryRevision)
			if event.Event != protocol.EventKindEntryUpdated || event.Data.Status == nil || *event.Data.Status != "usage_changed" {
				affected[*event.Data.EntryId] = struct{}{}
			}
		}
	}
	if coveredRevision != next.HistoryRevision {
		return nil, false
	}
	return affected, true
}

func (h *nativeHistory) reflow(state protocol.RuntimeState, affected map[int64]struct{}) {
	selected := h.selectedID()
	h.refreshLimit = max(pageSize, h.refreshLimit, len(h.rows))
	h.generation++
	h.state, h.more = state, false
	h.rows = slices.DeleteFunc(h.rows, func(row protocol.DesktopEntry) bool {
		_, changed := affected[row.Id]
		return changed
	})
	h.selectID(selected)
}

func (h *nativeHistory) selectedID() int64 {
	if h.selected < 0 || h.selected >= len(h.rows) {
		return 0
	}
	return h.rows[h.selected].Id
}

func (h *nativeHistory) selectID(id int64) {
	h.selected = -1
	if id != 0 {
		h.selected = slices.IndexFunc(h.rows, func(row protocol.DesktopEntry) bool { return row.Id == id })
	}
}

func (h *nativeHistory) nextOffset() uint32 {
	if h.refreshLimit > 0 {
		return 0
	}
	return uint32(len(h.rows))
}

// Collect a replacement for the loaded range before publishing any rows.
// Mixing page offsets from different revisions can skip or duplicate entries.
func readNativeHistoryRange(ctx context.Context, request nativeHistoryRequest, read func(context.Context, nativeHistoryRequest) (nativeHistoryPage, error)) (nativeHistoryPage, error) {
	page, err := read(ctx, request)
	if err != nil || request.refreshLimit == 0 {
		return page, err
	}
	for page.more && len(page.rows) < request.refreshLimit {
		request.offset = uint32(len(page.rows))
		next, err := read(ctx, request)
		if err != nil {
			return nativeHistoryPage{}, err
		}
		if next.state.ServiceInstanceId != page.state.ServiceInstanceId || next.state.PrivacyEpoch != page.state.PrivacyEpoch ||
			next.state.HistoryRevision != page.state.HistoryRevision || len(next.rows) == 0 && next.more {
			return nativeHistoryPage{}, client.ErrProtocol
		}
		page.state.EventSequence = max(page.state.EventSequence, next.state.EventSequence)
		page.rows = append(page.rows, next.rows...)
		page.more, page.total = next.more, next.total
	}
	return page, nil
}

func (h *nativeHistory) request() nativeHistoryRequest {
	return nativeHistoryRequest{
		generation: h.generation, state: h.state, filter: h.filter, offset: h.nextOffset(), refreshLimit: h.refreshLimit,
	}
}

func (h *nativeHistory) accepts(request nativeHistoryRequest) bool {
	return request.generation == h.generation && request.refreshLimit == h.refreshLimit && request.offset == h.nextOffset()
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
	h.state, h.more, h.total = page.state, page.more, page.total
	if request.refreshLimit > 0 {
		selected := h.selectedID()
		clear(h.rows)
		h.rows, h.refreshLimit = page.rows, 0
		h.selectID(selected)
	} else {
		h.rows = append(h.rows, page.rows...)
	}
	return true
}
