package main

import (
	"context"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestNativeTagDeletionKeepsBulkRevealAndSelection(t *testing.T) {
	removed, retained := nativeCardKey{id: 7}, nativeCardKey{id: 9}
	state := protocol.RuntimeState{ServiceInstanceId: "host-a", PrivacyEpoch: 4, HistoryRevision: 6}
	tags := &nativeTagLibrary{selected: "Work", sort: "time"}
	n := nativeMain{tagLibrary: tags}
	n.history.reset(state, n.historyFilter())
	tags.revealGeneration = n.history.generation
	tags.reveal = map[nativeCardKey]bool{removed: true, retained: true}
	tags.batch = nativeTagBatch{enabled: true, generation: n.history.generation,
		selection: nativeTagSelection{ids: map[int64]struct{}{7: {}, 9: {}}}}
	n.history.rows = []protocol.DesktopEntry{{Id: 7}, {Id: 9}}
	next := state
	next.HistoryRevision++
	next.EventSequence++
	id := int64(7)
	n.applyEvents(protocol.EventsResult{State: next, Events: []protocol.Event{{
		Event: protocol.EventKindEntryDeleted,
		State: next,
		Data:  protocol.EventData{EntryId: &id},
	}}})
	if len(n.history.rows) != 1 || n.history.rows[0].Id != 9 ||
		tags.revealGeneration != n.history.generation || !tags.reveal[retained] || tags.reveal[removed] ||
		tags.batch.generation != n.history.generation || !tags.batch.selection.includes(9) || tags.batch.selection.includes(7) {
		t.Fatalf("deletion reset live tag state: rows=%v, reveal=%v, batch=%+v", n.history.rows, tags.reveal, tags.batch)
	}
}

func TestNativeTagDeletionWithTagAssociationsKeepsRevealOnFullRefresh(t *testing.T) {
	deleted, excluded := nativeCardKey{id: 7}, nativeCardKey{id: 9}
	state := protocol.RuntimeState{ServiceInstanceId: "host-a", PrivacyEpoch: 4, HistoryRevision: 6}
	tags := &nativeTagLibrary{selected: "Work", sort: "time"}
	n := nativeMain{tagLibrary: tags}
	n.history.reset(state, n.historyFilter())
	tags.revealGeneration = n.history.generation
	tags.reveal = map[nativeCardKey]bool{deleted: true, excluded: true}
	n.history.rows = []protocol.DesktopEntry{{Id: 7}, {Id: 9}}
	next := state
	next.HistoryRevision += 2
	next.EventSequence++
	id := int64(7)
	n.applyEvents(protocol.EventsResult{State: next, Events: []protocol.Event{{
		Event: protocol.EventKindEntryDeleted,
		State: next,
		Data:  protocol.EventData{EntryId: &id},
	}}})
	if len(n.history.rows) != 0 || tags.revealGeneration != n.history.generation ||
		!tags.reveal[excluded] || tags.reveal[deleted] {
		t.Fatalf("tag reveal choice was lost in the full refresh: rows=%v, reveal=%v", n.history.rows, tags.reveal)
	}
}

func TestNativeTagBatchIncludesUnloadedMatchesAndPreservesExclusions(t *testing.T) {
	tags := &nativeTagLibrary{selected: "Work", query: "needle", sort: "time"}
	n := nativeMain{
		session: context.Background(), tagLibrary: tags,
		preferences: map[string]string{"app.language": "en", "app.arrow_key_selection": "true"},
	}
	n.tagCatalog.names, n.tagCatalog.counts, n.tagCatalog.fresh = []string{"Work"}, map[string]uint64{"Work": 185}, true
	n.history.reset(protocol.RuntimeState{ServiceInstanceId: "host-a", PrivacyEpoch: 4, HistoryRevision: 6}, n.historyFilter())
	n.history.rows = []protocol.DesktopEntry{
		{Id: 9, Preview: "A tagged row", ContentType: "text", Tags: []string{"Work"}},
		{Id: 11, Preview: "Another tagged row", ContentType: "text", Tags: []string{"Work"}},
	}
	n.history.total = 185
	n.cardTagColors = map[string]string{}
	view := ui.NewTester(n.view, 740, 480)
	click := func(label string) {
		t.Helper()
		if err := view.Click(label); err != nil {
			t.Fatalf("%v; visible text: %q", err, view.Texts())
		}
		view.Frame()
	}
	click("Manage")
	click("Select matches")
	if got := tags.batch.selection.count(n.history.total); got != 185 || !tags.batch.selection.includes(700) {
		t.Fatalf("select matches = %d; did not include unloaded rows", got)
	}
	click("Select entry: A tagged row")
	if tags.batch.selection.includes(9) || !tags.batch.selection.includes(11) {
		t.Fatal("toggling the row checkbox did not create its exclusion")
	}
	click("A tagged row")
	if !tags.batch.selection.includes(9) {
		t.Fatal("clicking the card did not restore its inclusion")
	}
	click("Select entry: A tagged row")
	view.Key(0, ui.KeyEnter)
	if !tags.batch.selection.includes(9) {
		t.Fatal("Enter did not toggle the focused checkbox")
	}
	view.Key(0, ui.KeyEnter)
	if tags.batch.selection.includes(9) {
		t.Fatal("Enter toggled something other than the focused checkbox")
	}
	n.history.selected, n.keyboard = 0, true
	view.Frame()
	view.Key(0, ui.KeyDown)
	if n.history.selected != 1 {
		t.Fatal("Down did not move to the next tag entry")
	}
	view.Frame()
	view.Key(0, ui.KeyEnter)
	if tags.batch.selection.includes(11) {
		t.Fatalf("Enter did not toggle the selected row while managing: exclusions = %v", tags.batch.selection.ids)
	}
	view.Key(0, ui.KeyEnter)
	if !tags.batch.selection.includes(11) {
		t.Fatal("Enter did not restore the selected row")
	}
	click("Delete")
	dialog := tags.batch.pending
	if dialog == nil {
		t.Fatal("delete skipped confirmation")
	}
	p := dialog.params
	if p.Name != "Work" || p.Scope != "all" || p.Query == nil || *p.Query != "needle" ||
		!slices.Equal(p.Ids, []int64{9}) || p.ServiceInstanceId != "host-a" || p.ExpectedEpoch != 4 {
		t.Fatalf("wrong batch request: %+v", p)
	}
	// A changed history must return to a fresh selection before a delete is sent.
	n.history.state.HistoryRevision++
	n.deleteTagBatch(tags, dialog)
	if dialog.open || n.mutating || tags.batch.notice != n.text("tag_selection_changed") ||
		tags.batch.selection.count(n.history.total) != 0 {
		t.Fatal("the stale confirmation remained actionable")
	}
}

func TestNativeTagEntryDeleteRequiresConfirmation(t *testing.T) {
	tags := &nativeTagLibrary{selected: "Work", sort: "time"}
	n := nativeMain{
		session: context.Background(), tagLibrary: tags,
		preferences: map[string]string{"app.language": "en", "app.arrow_key_selection": "true"},
	}
	n.tagCatalog.fresh = true
	n.history.reset(protocol.RuntimeState{ServiceInstanceId: "fixture", PrivacyEpoch: 1}, n.historyFilter())
	n.history.rows = []protocol.DesktopEntry{{Id: 9, ContentType: "text", Preview: "Tagged fixture", Tags: []string{"Work"}}}
	n.history.total = 1
	n.cardTagColors = map[string]string{}
	revealed := nativeCardKey{id: 9}
	tags.revealGeneration = n.history.generation
	tags.reveal = map[nativeCardKey]bool{revealed: true}

	view := ui.NewTester(n.view, 740, 400)
	view.Key(0, ui.KeyDown)
	view.Key(0, ui.KeyDelete)
	if tags.entryDelete == nil || tags.entryDelete.row.Id != 9 || n.mutating {
		t.Fatal("Delete did not request confirmation for the selected row")
	}
	if !tags.reveal[revealed] {
		t.Fatal("Opening the confirmation turned off Reveal All")
	}
	view.Frame()
	if !view.HasText("Are you sure you want to delete this item?") ||
		!view.HasText("Are you sure you want to delete this record?") {
		t.Fatal("the tag entry confirmation did not show its title and message")
	}
	if err := view.Click(n.text("common_cancel")); err != nil || tags.entryDelete != nil || n.mutating {
		t.Fatalf("Cancel did not dismiss the delete request without mutating history: %v", err)
	}
	if !tags.reveal[revealed] {
		t.Fatal("Cancelling the confirmation turned off Reveal All")
	}

	row := n.history.rows[0]
	n.requestRowDeletion(row, n.history.generation)
	dialog := tags.entryDelete
	n.history.generation++
	n.confirmTagEntryDelete(tags, dialog)
	if dialog.open || n.mutating || n.notice != n.text("tag_selection_changed") {
		t.Fatal("an obsolete selection could still dispatch a deletion")
	}
}

func TestNativeHiddenTagLibraryKeepsNavigationButDismissesActions(t *testing.T) {
	tags := &nativeTagLibrary{
		selected: "Work", sort: "count", query: "meeting", nameQuery: "work",
		railCollapsed: true, batch: nativeTagBatch{enabled: true},
		entryDelete: &nativeTagEntryDeletion{open: true},
		reveal:      map[nativeCardKey]bool{{id: 7}: true},
	}
	n := nativeMain{tagLibrary: tags}
	n.stopSession()
	restored := n.tagLibrary
	if restored == nil || restored.selected != "Work" || restored.sort != "count" ||
		restored.query != "meeting" || restored.nameQuery != "work" || !restored.railCollapsed {
		t.Fatalf("tag navigation was lost when retiring the visible session: %+v", restored)
	}
	if restored.batch.enabled || restored.entryDelete != nil || restored.reveal != nil {
		t.Fatal("an action or reveal survived beyond its presentation session")
	}
}
