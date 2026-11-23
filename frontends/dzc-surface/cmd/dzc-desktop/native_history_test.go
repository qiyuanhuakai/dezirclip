package main

import (
	"context"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestNativePreviewRecoversReadableTextAndKeepsProtectedRowsHidden(t *testing.T) {
	const invisible = "\u200b\u200e\ufeff\uFE0F\t"
	const body = "真实文本"
	n := nativeMain{preferences: map[string]string{"app.language": "en"}}
	row := protocol.DesktopEntry{Id: 2, Timestamp: 10, ContentType: "text", Tags: []string{}, Preview: invisible}
	row.Content = invisible + strings.Repeat(body, 200)
	if summary, placeholder := n.cardPresentation(row, nil); !strings.HasPrefix(summary, body) ||
		len([]rune(summary)) != 500 || placeholder != "" {
		t.Fatalf("expected readable text capped at 500 runes, got %d runes and %q", len([]rune(summary)), placeholder)
	}
	row.Content = ""
	if summary, placeholder := n.cardPresentation(row, nil); summary != "" || placeholder != n.text("clipboard_preview_empty") {
		t.Fatalf("empty card: %q, %q", summary, placeholder)
	}
	n.session, n.library = context.Background(), &Library{}
	n.history.state.ServiceInstanceId = "preview-fixture"
	key := nativeCardKey{row.Id, row.Timestamp}
	n.cardMedia.begin(key)
	if _, placeholder := n.cardPresentation(row, nil); placeholder != n.text("clipboard_preview_loading") {
		t.Fatalf("pending card: %q", placeholder)
	}
	n.cardMedia.finish(key, nativeCardMedia{failed: true})
	if _, placeholder := n.cardPresentation(row, nil); placeholder != n.text("clipboard_preview_unavailable") {
		t.Fatalf("failed card: %q", placeholder)
	}
	n.cardMedia.finish(key, nativeCardMedia{summary: body})
	if summary, placeholder := n.cardPresentation(row, nil); summary != body || placeholder != "" {
		t.Fatalf("hydrated card: %q, %q", summary, placeholder)
	}
	row.Tags = nil
	if summary, placeholder := n.cardPresentation(row, nil); summary != "Sensitive content hidden" || placeholder != "" {
		t.Fatalf("protected preview reused the public cache: %q, %q", summary, placeholder)
	}
}

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

func TestNativeHistoryReflowsWithoutBlankingUnchangedCards(t *testing.T) {
	const untouched = "The card that stays visible"
	state := protocol.RuntimeState{ServiceInstanceId: "host-a", PrivacyEpoch: 7, HistoryRevision: 10}
	n := nativeMain{preferences: map[string]string{"app.language": "en"}}
	n.history.reset(state, nativeHistoryFilter{})
	n.history.rows = []protocol.DesktopEntry{
		{Id: 1, ContentType: "text", Preview: "Delete this card", Tags: []string{}},
		{Id: 2, ContentType: "text", Preview: untouched, Tags: []string{}},
		{Id: 3, ContentType: "text", Preview: "Paste this card", Tags: []string{}},
	}
	n.history.selected = 1
	view := ui.NewTester(n.view, 400, 480)
	if !view.HasText(untouched) {
		t.Fatal("fixture card is not visible")
	}
	oldRead := n.history.request()
	state.HistoryRevision++
	deleteID, pasteID, usage := int64(1), int64(3), "usage_changed"
	n.applyEvents(protocol.EventsResult{State: state, Events: []protocol.Event{
		{Event: protocol.EventKindEntryDeleted, State: state, Data: protocol.EventData{EntryId: &deleteID}},
		{Event: protocol.EventKindEntryUpdated, State: state, Data: protocol.EventData{EntryId: &pasteID, Status: &usage}},
	}})
	n.reading = true
	view.Frame()
	if !view.HasText(untouched) || !view.HasText("Paste this card") || view.HasText("Delete this card") {
		t.Fatalf("visible cards were not reconciled immediately: %v", view.Texts())
	}
	if n.history.accepts(oldRead) || n.history.request().offset != 0 {
		t.Fatal("refresh did not retire requests using the old offset")
	}
	reordered := []protocol.DesktopEntry{n.history.rows[1], n.history.rows[0]}
	if !n.history.apply(n.history.request(), nativeHistoryPage{state: state, rows: reordered}) || n.history.selected != 1 {
		t.Fatal("the replacement lost the selected entry or used the wrong order")
	}
	n.reading = false
	view.Frame()
	if !view.HasText(untouched) || view.HasText("Delete this card") {
		t.Fatal("the refreshed view lost an unchanged entry or brought a deleted one back")
	}
}

func TestNativeHistoryKeepsNoRowsAcrossUnknownRevisions(t *testing.T) {
	current := protocol.RuntimeState{ServiceInstanceId: "host-a", PrivacyEpoch: 7, HistoryRevision: 10}
	next := current
	next.HistoryRevision += 2
	id := int64(3)
	if _, ok := historyRefreshEntries(current, protocol.EventsResult{State: next, Events: []protocol.Event{
		{Event: protocol.EventKindEntryUpdated, State: next, Data: protocol.EventData{EntryId: &id}},
	}}); ok {
		t.Fatal("an unaccounted-for revision permitted stale rows to remain visible")
	}
	next.HistoryRevision--
	if affected, ok := historyRefreshEntries(current, protocol.EventsResult{State: next, Events: []protocol.Event{
		{Event: protocol.EventKindEntryUpdated, State: next, Data: protocol.EventData{EntryId: &id}},
	}}); !ok || len(affected) != 1 {
		t.Fatal("a changed entry must be dropped pending its new authorized body")
	}
	next.PrivacyEpoch++
	if _, ok := historyRefreshEntries(current, protocol.EventsResult{State: next}); ok {
		t.Fatal("a privacy epoch change permitted stale rows to remain visible")
	}
}

func TestNativeHistoryRangeDoesNotMixRevisions(t *testing.T) {
	state := protocol.RuntimeState{ServiceInstanceId: "host-a", PrivacyEpoch: 7, HistoryRevision: 20}
	for _, drift := range []bool{false, true} {
		request := nativeHistoryRequest{state: state, refreshLimit: 2}
		page, err := readNativeHistoryRange(context.Background(), request, func(_ context.Context, request nativeHistoryRequest) (nativeHistoryPage, error) {
			next := state
			if request.offset == 1 && drift {
				next.HistoryRevision++
			}
			return nativeHistoryPage{state: next, rows: []protocol.DesktopEntry{{Id: int64(request.offset) + 1}}, more: request.offset == 0}, nil
		})
		if drift && err == nil || !drift && (err != nil || len(page.rows) != 2 || page.rows[1].Id != 2) {
			t.Fatalf("revision drift=%t: collected %+v, err=%v", drift, page, err)
		}
	}
}

func TestNativePreviewOpacityPreservesLivePresentation(t *testing.T) {
	current := map[string]string{"app.theme": "macos", "app.preview_opacity": "65"}
	next := map[string]string{"app.theme": "macos", "app.preview_opacity": "24"}
	if !sameSettingsExceptPreviewOpacity(current, next) ||
		sameSettingsExceptPreviewOpacity(current, map[string]string{"app.theme": "mica", "app.preview_opacity": "24"}) {
		t.Fatal("view settings comparison did not isolate preview opacity")
	}
	session := context.Background()
	control := &previewOpacityControl{value: 65, committed: 65, slider: 65}
	n := nativeMain{session: session, preferences: current, preview: &Preview{entryID: 7, opacity: control}}
	n.history.rows = []protocol.DesktopEntry{{Id: 7, Preview: "Still visible"}}
	n.history.generation = 2
	n.applyPreviewViewSettings(next)
	if n.session != session || n.history.generation != 2 || len(n.history.rows) != 1 || n.preview.entryID != 7 {
		t.Fatal("preview opacity restarted the live presentation")
	}
	if control.value != 24 || control.committed != 24 || current["app.preview_opacity"] != "65" {
		t.Fatal("preview opacity did not apply in place, or rewrote an existing preference snapshot")
	}
}
