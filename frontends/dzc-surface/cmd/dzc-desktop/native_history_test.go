package main

import (
	"testing"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

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
