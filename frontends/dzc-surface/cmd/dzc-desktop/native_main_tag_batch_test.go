package main

import (
	"context"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

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
