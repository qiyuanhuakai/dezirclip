package main

import (
	"context"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

type nativeHistoryRequest struct {
	generation uint64
	state      protocol.RuntimeState
	query      string
	kind       string
	offset     uint32
}

type nativeHistoryPage struct {
	state protocol.RuntimeState
	rows  []protocol.DesktopEntry
	more  bool
}

func (l *Library) nativeHistoryPage(ctx context.Context, request nativeHistoryRequest) (nativeHistoryPage, error) {
	params := protocol.DesktopPageParams{Offset: request.offset, Limit: pageSize + 1}
	if request.query != "" {
		params.Query = &request.query
	}
	if request.kind != "" {
		params.Kind = &request.kind
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

// nativeHistory owns the rows for one live main-window presentation. A
// generation change retires pending reads, including a search that comes
// back to the same query after the user has already dismissed it.
type nativeHistory struct {
	state      protocol.RuntimeState
	generation uint64
	query      string
	kind       string
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

func (h *nativeHistory) reset(state protocol.RuntimeState, query, kind string) {
	h.clear()
	h.state, h.query, h.kind = state, query, kind
}

func (h *nativeHistory) request() nativeHistoryRequest {
	return nativeHistoryRequest{
		generation: h.generation, state: h.state, query: h.query, kind: h.kind, offset: uint32(len(h.rows)),
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
