package quickpaste

import (
	"testing"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/model"
)

func TestScrollPrefetchIsBoundedAndPreservesSelection(t *testing.T) {
	m := model.Model{Visible: true, Session: "session", Display: "display", Selected: 47,
		HasMore: true, NextOffset: 50, Entries: make([]protocol.EntrySummary, 50)}
	for i := range m.Entries {
		m.Entries[i].Id = int64(i + 1)
	}
	var prefetch pagePrefetch
	var queued []func()
	post := func(fn func()) { queued = append(queued, fn) }
	var request model.Ticket
	reads := 0
	read := func(offset uint32) { reads++; request = m.Begin(offset) }
	for _, last := range []int{-1, 5, 46} {
		prefetch.nearEnd(&m, last, post, read)
	}
	if len(queued) != 0 {
		t.Fatal("prefetched before nearing the end")
	}
	for range 5 {
		prefetch.nearEnd(&m, 47, post, read)
	}
	if len(queued) != 1 || reads != 0 || m.Loading {
		t.Fatal("render mutated the list or queued duplicate reads")
	}
	queued[0]()
	if reads != 1 || request.Offset != 50 || len(m.Entries) != 50 || m.Selected != 47 {
		t.Fatal("append lost position or selection")
	}
	prefetch.nearEnd(&m, 49, post, read)
	if len(queued) != 1 {
		t.Fatal("duplicated in-flight request")
	}
	if !m.Apply(request, protocol.SurfacePage{State: m.State, Entries: []protocol.EntrySummary{{Id: 51}}}) {
		t.Fatal("append rejected")
	}
	prefetch.nearEnd(&m, 50, post, read)
	if len(queued) != 1 || m.Selected != 47 {
		t.Fatal("requested past EOF or changed selection")
	}
}

func TestQueuedPrefetchCannotSurviveViewChanges(t *testing.T) {
	for name, change := range map[string]func(*model.Model){
		"query":       func(m *model.Model) { m.Edit("new", false) },
		"hidden":      func(m *model.Model) { m.Clear() },
		"epoch":       func(m *model.Model) { m.State.PrivacyEpoch++ },
		"history":     func(m *model.Model) { m.State.HistoryRevision++ },
		"paste":       func(m *model.Model) { m.PendingAction = "paste" },
		"composition": func(m *model.Model) { m.Composing = true },
		"failure":     func(m *model.Model) { m.Error = "read failed" },
	} {
		t.Run(name, func(t *testing.T) {
			m := model.Model{Visible: true, HasMore: true, NextOffset: 1, Entries: []protocol.EntrySummary{{Id: 1}}}
			var prefetch pagePrefetch
			var queued func()
			prefetch.nearEnd(&m, 0, func(fn func()) { queued = fn }, func(uint32) { t.Fatal("stale append executed") })
			if queued == nil {
				t.Fatal("expected queued append")
			}
			change(&m)
			queued()
			if prefetch.queued {
				t.Fatal("cancelled append kept its queue slot")
			}
		})
	}
}
