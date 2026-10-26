package model

import (
	"math"
	"strings"
	"testing"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func state() protocol.RuntimeState {
	return protocol.RuntimeState{ServiceInstanceId: "11111111-1111-4111-8111-111111111111", PrivacyEpoch: 2, HistoryRevision: 3, EventSequence: 4}
}
func row(id int64) protocol.EntrySummary {
	text := "Known summary"
	return protocol.EntrySummary{Id: id, Kind: "text", Preview: &text, PrivacyVisibility: "ordinary"}
}
func ready(t *testing.T) Model {
	t.Helper()
	var view Model
	view.Show("session", "display", state())
	ticket := view.Begin(0)
	if !view.Apply(ticket, protocol.SurfacePage{Entries: []protocol.EntrySummary{row(1)}, State: state()}) {
		t.Fatal("initial page rejected")
	}
	return view
}

func TestChangedQueryImmediatelyRevokesSelection(t *testing.T) {
	view := ready(t)
	if !view.CanSelect() {
		t.Fatal("ready row cannot be selected")
	}
	view.Edit("unmatched", false)
	if view.CanSelect() || len(view.Entries) != 0 {
		t.Fatal("old query is actionable during debounce")
	}
	view.Begin(0)
	if view.CanSelect() {
		t.Fatal("query dispatch must not reactivate old rows")
	}
}

func TestReturningToSameQueryDoesNotReviveOldRequest(t *testing.T) {
	view := ready(t)
	view.Edit("A", false)
	old := view.Begin(0)
	view.Edit("B", false)
	view.Begin(0)
	view.Edit("A", false)
	latest := view.Begin(0)
	page := protocol.SurfacePage{Entries: []protocol.EntrySummary{row(2)}, State: state()}
	if view.Apply(old, page) {
		t.Fatal("old A revived")
	}
	if !view.Apply(latest, page) || !view.CanSelect() {
		t.Fatal("current A not delivered")
	}
}

func TestEveryDispatchInvalidatesPriorSameQueryDelivery(t *testing.T) {
	var view Model
	view.Show("s", "d", state())
	old, latest := view.Begin(0), view.Begin(0)
	page := protocol.SurfacePage{Entries: []protocol.EntrySummary{row(1)}, State: state()}
	if view.Apply(old, page) || !view.Apply(latest, page) {
		t.Fatal("overlapping requests were not sequenced")
	}
}

func TestHideAndNewDisplayRejectLateResults(t *testing.T) {
	view := ready(t)
	old := view.Begin(1)
	view.Clear()
	if view.Visible || len(view.Entries) != 0 || view.PendingAction != "" {
		t.Fatal("hide retained view state")
	}
	view.Show("session", "new-display", state())
	if view.Apply(old, protocol.SurfacePage{Entries: []protocol.EntrySummary{row(2)}, State: state()}) {
		t.Fatal("old display revived")
	}
}

func TestHostEpochRevisionAndBackwardsSequenceFence(t *testing.T) {
	for _, field := range []string{"host", "epoch", "revision", "sequence"} {
		t.Run(field, func(t *testing.T) {
			var view Model
			view.Show("s", "d", state())
			ticket := view.Begin(0)
			changed := state()
			switch field {
			case "host":
				changed.ServiceInstanceId = "another"
			case "epoch":
				changed.PrivacyEpoch++
			case "revision":
				changed.HistoryRevision++
			case "sequence":
				changed.EventSequence--
			}
			if view.Apply(ticket, protocol.SurfacePage{Entries: []protocol.EntrySummary{row(1)}, State: changed}) {
				t.Fatal("changed state delivered")
			}
		})
	}
}

func TestIMEAndPendingActionCannotPaste(t *testing.T) {
	view := ready(t)
	view.Composing = true
	if view.CanSelect() {
		t.Fatal("composition can submit")
	}
	view.Composing = false
	view.PendingAction = "original-action"
	if view.CanSelect() {
		t.Fatal("an uncertain action can be repeated")
	}
}

func TestPaginationRequiresContiguousUniqueOriginalStateRows(t *testing.T) {
	view := ready(t)
	ticket := view.Begin(1)
	next := uint32(2)
	if !view.Apply(ticket, protocol.SurfacePage{Entries: []protocol.EntrySummary{row(2)}, State: state(), HasMore: true, NextOffset: &next}) {
		t.Fatal("valid page rejected")
	}
	if len(view.Entries) != 2 || !view.HasMore || view.NextOffset != 2 {
		t.Fatal("page not appended")
	}
	ticket = view.Begin(2)
	if view.Apply(ticket, protocol.SurfacePage{Entries: []protocol.EntrySummary{row(1)}, State: state()}) {
		t.Fatal("duplicate action ID accepted")
	}
}

func TestMalformedPaginationDoesNotMutateRows(t *testing.T) {
	view := ready(t)
	ticket := view.Begin(1)
	next := uint32(50)
	if view.Apply(ticket, protocol.SurfacePage{Entries: []protocol.EntrySummary{row(2)}, State: state(), HasMore: true, NextOffset: &next}) || len(view.Entries) != 1 {
		t.Fatal("bad next offset changed rows")
	}
}

func TestMaskedSummaryNeverUsesProtectedPreview(t *testing.T) {
	entry := row(1)
	entry.PrivacyVisibility = "masked"
	if DisplayText(entry, "Hidden") != "Hidden" {
		t.Fatal("masked preview rendered")
	}
	entry.PrivacyVisibility = "ordinary"
	text := "a\n" + strings.Repeat("界", 300)
	entry.Preview = &text
	got := DisplayText(entry, "Hidden")
	if strings.Contains(got, "\n") || len([]rune(got)) != 241 {
		t.Fatal("summary rendering is unbounded")
	}
}

func TestPhysicalCoordinatesMapNegativeOriginsAndDPI(t *testing.T) {
	bounds, ok := LogicalBounds(protocol.SurfacePhysicalRect{X: -1200, Y: -150, Width: 840, Height: 660}, 1.5)
	if !ok || bounds != (Bounds{X: -800, Y: -100, Width: 560, Height: 440}) {
		t.Fatalf("wrong DPI conversion: %+v", bounds)
	}
}

func TestInvalidWindowGeometryIsRejected(t *testing.T) {
	for _, scale := range []float64{0, -1, math.NaN(), math.Inf(1), 100} {
		if _, ok := LogicalBounds(protocol.SurfacePhysicalRect{Width: 560, Height: 440}, scale); ok {
			t.Fatalf("accepted scale %v", scale)
		}
	}
	for _, rect := range []protocol.SurfacePhysicalRect{{Width: 0, Height: 1}, {X: math.MaxInt32 + 1, Width: 1, Height: 1}, {Width: 20000, Height: 1}} {
		if _, ok := LogicalBounds(rect, 1); ok {
			t.Fatalf("accepted rectangle %+v", rect)
		}
	}
}

func TestTraditionalChineseUsesNativeLocaleIdentifier(t *testing.T) {
	if NativeLocale("tw") != "zh-TW" || NativeLocale("zh") != "zh" || NativeLocale("en") != "en" {
		t.Fatal("invalid native locale mapping")
	}
}

func TestPasteNoticeSurvivesQueryButCannotCrossDisplay(t *testing.T) {
	view := ready(t)
	view.ShowNotice = true
	view.Edit("new", false)
	ticket := view.Begin(0)
	if !view.Apply(ticket, protocol.SurfacePage{Entries: []protocol.EntrySummary{row(2)}, State: state()}) || !view.ShowNotice {
		t.Fatal("query erased the pending user notice")
	}
	view.Clear()
	if view.ShowNotice {
		t.Fatal("hidden view retained an old notice")
	}
	view.Show("session", "new-display", state())
	if view.ShowNotice {
		t.Fatal("old notice appeared in an unrelated display")
	}
}
