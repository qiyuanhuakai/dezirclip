package model

import (
	"testing"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestViewRowsDetachFromMutableSummary(t *testing.T) {
	m := ready(t)
	m.Entries[0].SourceApp = "Original source"
	v := m.CaptureView("Protected")
	*m.Entries[0].Preview = "Changed preview"
	m.Entries[0].SourceApp = "Changed source"
	got, ok := v.Row(&m, 0)
	if !ok || got.Text != "Known summary" || got.SourceApp != "Original source" || got.ID != 1 {
		t.Fatal("a built frame followed mutable summary fields")
	}
}

func TestViewRevocationClearsStoredValues(t *testing.T) {
	m := ready(t)
	v := m.CaptureView("Protected")
	stored := v.rows
	v.Revoke()
	v.Revoke()
	if v.rows != nil || v.ticket != (Ticket{}) || v.ready || !v.revoked || stored[0] != (ViewRow{}) {
		t.Fatal("revocation retained frame metadata")
	}
	if _, ok := v.Row(&m, 0); ok {
		t.Fatal("revoked frame rendered a row")
	}
	if _, ok := v.SelectedID(&m); ok || v.Len(&m) != 0 {
		t.Fatal("revoked frame remained actionable")
	}
}

func TestViewRejectsEveryChangedGeneration(t *testing.T) {
	tests := []struct {
		name   string
		change func(*Model)
	}{
		{"session", func(m *Model) { m.Session = "other-session" }},
		{"display", func(m *Model) { m.Display = "other-display" }},
		{"sequence", func(m *Model) { m.Sequence++ }},
		{"query", func(m *Model) { m.RawQuery = "other-query" }},
		{"service", func(m *Model) { m.State.ServiceInstanceId = "other-service" }},
		{"privacy", func(m *Model) { m.State.PrivacyEpoch++ }},
		{"history", func(m *Model) { m.State.HistoryRevision++ }},
		{"hidden", func(m *Model) { m.Visible = false }},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			m := ready(t)
			v := m.CaptureView("Protected")
			test.change(&m)
			if v.Current(&m) || v.rows != nil || v.ticket != (Ticket{}) {
				t.Fatal("an obsolete frame retained or exposed its values")
			}
			if _, ok := v.SelectedID(&m); ok {
				t.Fatal("an obsolete callback selected current rows")
			}
		})
	}
}

func TestViewSameQueryDoesNotReviveOldFrame(t *testing.T) {
	m := ready(t)
	v := m.CaptureView("Protected")
	m.Edit("other", false)
	m.Edit("", false)
	if v.Current(&m) {
		t.Fatal("returning to the same query revived an older frame")
	}
}

func TestLoadingViewCannotSelectNewlyDeliveredRows(t *testing.T) {
	var m Model
	m.Show("session", "display", state())
	ticket := m.Begin(0)
	v := m.CaptureView("Protected")
	if !m.Apply(ticket, protocol.SurfacePage{Entries: []protocol.EntrySummary{row(1)}, State: state()}) || !m.CanSelect() {
		t.Fatal("current page did not become ready")
	}
	if !v.Current(&m) {
		t.Fatal("page delivery unexpectedly changed the query generation")
	}
	if _, ok := v.SelectedID(&m); ok {
		t.Fatal("an old loading callback selected an unseen delivered row")
	}
}

func TestPendingViewCannotReviveAfterFailedPaste(t *testing.T) {
	m := ready(t)
	m.PendingAction = "pending"
	v := m.CaptureView("Protected")
	m.PendingAction = ""
	if _, ok := v.SelectedID(&m); ok {
		t.Fatal("a disabled frame became actionable without being rebuilt")
	}
}

func TestViewSelectionChecksCapturedIdentity(t *testing.T) {
	m := ready(t)
	m.Entries = append(m.Entries, row(2))
	v := m.CaptureView("Protected")
	m.Selected = 1
	selected, ok := v.SelectedID(&m)
	if !ok || selected != 2 {
		t.Fatal("a current keyboard selection did not use its displayed identity")
	}
	m.Entries[1] = row(3)
	if _, ok := v.SelectedID(&m); ok {
		t.Fatal("a captured callback selected a different live row at the same index")
	}
}

func TestViewRowBoundsAndNilAreInert(t *testing.T) {
	m := ready(t)
	v := m.CaptureView("Protected")
	for _, index := range []int{-1, 1, MaxVisibleEntries} {
		if _, ok := v.Row(&m, index); ok {
			t.Fatal("out-of-frame index returned a row")
		}
	}
	var missing *View
	missing.Revoke()
	if missing.Current(&m) || missing.Len(&m) != 0 || v.Current(nil) {
		t.Fatal("missing view or model remained current")
	}
	if _, ok := missing.SelectedID(&m); ok {
		t.Fatal("missing frame selected a row")
	}
}

func TestViewDoesNotStoreProtectedPreview(t *testing.T) {
	m := ready(t)
	*m.Entries[0].Preview = "Protected body must stay unavailable"
	m.Entries[0].PrivacyVisibility = "masked"
	v := m.CaptureView("Protected")
	got, ok := v.Row(&m, 0)
	if !ok || got.Text != "Protected" {
		t.Fatal("a protected summary exposed its preview through the frame")
	}
}
