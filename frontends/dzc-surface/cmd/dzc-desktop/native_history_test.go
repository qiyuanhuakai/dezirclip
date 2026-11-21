package main

import (
	"context"
	"strings"
	"testing"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestNativeProtectedRowsExpireOutsideTheirVisibleVersion(t *testing.T) {
	visible := protocol.DesktopEntry{Id: 3, Timestamp: 22}
	changed := protocol.DesktopEntry{Id: 4, Timestamp: 32}
	offscreen := protocol.DesktopEntry{Id: 5, Timestamp: 42}
	n := nativeMain{history: nativeHistory{rows: []protocol.DesktopEntry{visible, changed, offscreen}}}
	n.protectedRows = make(map[nativeCardKey]*nativeProtectedRow)
	add := func(key nativeCardKey) context.Context {
		ctx, cancel := context.WithCancel(context.Background())
		t.Cleanup(cancel)
		n.protectedRows[key] = &nativeProtectedRow{cancel: cancel, body: &nativeProtectedBody{}}
		return ctx
	}
	shown := add(nativeCardKey{visible.Id, visible.Timestamp})
	previous := add(nativeCardKey{changed.Id, changed.Timestamp - 1})
	hidden := add(nativeCardKey{offscreen.Id, offscreen.Timestamp})

	n.pruneProtectedRows(0, 1)
	if shown.Err() != nil || n.protectedBody(visible) == nil {
		t.Fatal("the visible row lost its reveal")
	}
	if previous.Err() == nil || hidden.Err() == nil || n.protectedBody(changed) != nil || n.protectedBody(offscreen) != nil {
		t.Fatal("a stale or offscreen reveal survived")
	}
	n.resetPreviewSelection()
	if shown.Err() == nil || n.protectedBody(visible) != nil {
		t.Fatal("the presentation reset kept protected content")
	}
}

func TestNativeHistoryOCRUsesTheCurrentAuthorizedImage(t *testing.T) {
	status := "done"
	row := protocol.DesktopEntry{Id: 3, Timestamp: 22, ContentType: "image", OcrStatus: &status}
	n := nativeMain{ocrPreview: &nativeOCRPreview{key: nativeCardKey{3, 22}, text: "Invoice 0842"}}
	if badge, result := n.cardOCR(row, nil, 0); badge != nil || result != nil {
		t.Fatal("a concealed image exposed its recognition status or text")
	}
	body := &nativeProtectedBody{ocrStatus: "done"}
	if badge, result := n.cardOCR(row, body, 0); badge == nil || result == nil || !result.Expanded || result.Text != "Invoice 0842" {
		t.Fatal("the current revealed image lost its recognition result")
	}
	row.Timestamp++
	if _, result := n.cardOCR(row, body, 0); result == nil || result.Expanded || result.Text != "" {
		t.Fatal("an image revision inherited previous recognition text")
	}
}

func TestNativeMainHistoryFilterMatchesTagSearch(t *testing.T) {
	window := nativeMain{query: "  TaG:  Work notes  ", kind: "text"}
	if got := window.historyFilter(); got != (nativeHistoryFilter{tag: "Work notes", kind: "text", sort: "pinned"}) {
		t.Fatalf("tag-only search: %+v", got)
	}
	window.query = "tag: "
	if got := window.historyFilter(); got != (nativeHistoryFilter{kind: "text", sort: "pinned"}) {
		t.Fatalf("empty tag selection: %+v", got)
	}
	window.query = " note "
	if got := window.historyFilter(); got != (nativeHistoryFilter{query: "note", kind: "text"}) {
		t.Fatalf("text search: %+v", got)
	}
	window.tagLibrary = &nativeTagLibrary{selected: "Work notes", query: strings.Repeat("測", 280), sort: "count"}
	if got := window.historyFilter(); got != (nativeHistoryFilter{tag: "Work notes", query: strings.Repeat("測", 256), sort: "count"}) {
		t.Fatalf("tag-content search: %+v", got)
	}
}

func TestNativeHistoryFencesRowsAcrossPresentationAndPrivacyChanges(t *testing.T) {
	state := protocol.RuntimeState{ServiceInstanceId: "host-a", PrivacyEpoch: 7, HistoryRevision: 10}
	var history nativeHistory
	filter := nativeHistoryFilter{query: "clipboard", kind: "text"}
	history.reset(state, filter)
	first := history.request()
	initialPage := nativeHistoryPage{state: state, rows: []protocol.DesktopEntry{{Id: 1, Preview: "Fixture"}}, more: true}
	if !history.apply(first, initialPage) || len(history.rows) != 1 || !history.more {
		t.Fatal("fresh first page did not commit")
	}
	next := history.request()
	drifted := nativeHistoryPage{state: state, rows: []protocol.DesktopEntry{{Id: 2}}}
	drifted.state.HistoryRevision++
	if history.apply(next, drifted) || len(history.rows) != 1 {
		t.Fatal("page with a moved cursor committed over the previous revision")
	}

	for _, changed := range []protocol.RuntimeState{
		{ServiceInstanceId: "host-b", PrivacyEpoch: state.PrivacyEpoch, HistoryRevision: state.HistoryRevision},
		{ServiceInstanceId: state.ServiceInstanceId, PrivacyEpoch: state.PrivacyEpoch + 1, HistoryRevision: state.HistoryRevision},
		state,
	} {
		history.reset(changed, filter)
		if history.apply(first, initialPage) || len(history.rows) != 0 || history.selected != -1 {
			t.Fatalf("previous presentation committed after reset to %+v", changed)
		}
	}
}
