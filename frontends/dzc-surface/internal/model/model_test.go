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

func hundredRows(t *testing.T) Model {
	t.Helper()
	var view Model
	view.Show("session", "display", state())
	for offset := 0; offset < 100; offset += PageSize {
		entries := make([]protocol.EntrySummary, PageSize)
		for i := range entries {
			entries[i] = row(int64(offset + i + 1))
		}
		next := uint32(offset + len(entries))
		ticket := view.Begin(uint32(offset))
		if !view.Apply(ticket, protocol.SurfacePage{Entries: entries, State: state(), HasMore: true, NextOffset: &next}) {
			t.Fatal("initial pagination rejected")
		}
	}
	view.Selected = 75
	return view
}

func firstPage() protocol.SurfacePage {
	entries := make([]protocol.EntrySummary, PageSize)
	for i := range entries {
		entries[i] = row(int64(i + 1))
	}
	next := uint32(PageSize)
	return protocol.SurfacePage{Entries: entries, State: state(), HasMore: true, NextOffset: &next}
}

func TestPaginationFailureRetryRestoresKeyboardSelection(t *testing.T) {
	view := hundredRows(t)
	failed := view.Begin(100)
	if view.CanSelect() || !view.Fail(failed, "read_failed") {
		t.Fatal("failed page was not fenced")
	}
	if len(view.Entries) != 0 || view.Selected != 0 || view.HasMore || view.NextOffset != 0 || view.AppliedQuery != "" || view.Loading || view.Error != "read_failed" || view.CanSelect() {
		t.Fatal("failure retained an actionable page or old selection")
	}
	retry := view.Begin(0)
	if view.CanSelect() || view.Error != "" || !view.Loading {
		t.Fatal("retry was selectable before receiving its page")
	}
	if !view.Apply(retry, firstPage()) || view.Selected != 0 || !view.CanSelect() {
		t.Fatal("first-page retry did not recover a valid selected row")
	}
	view.Move(1)
	if view.Selected != 1 || view.Entries[view.Selected].Id != 2 || !view.CanSelect() {
		t.Fatal("keyboard navigation remained stuck after retry")
	}
}

func TestFirstPageReloadResetsResultsBeforeDelivery(t *testing.T) {
	view := hundredRows(t)
	view.ShowNotice = true
	ticket := view.Begin(0)
	if len(view.Entries) != 0 || view.Selected != 0 || view.NextOffset != 0 || view.HasMore || !view.Loading || view.CanSelect() || !view.ShowNotice {
		t.Fatal("first-page reload retained old page state or erased its notice")
	}
	if !view.Apply(ticket, firstPage()) || view.Selected != 0 || len(view.Entries) != PageSize || !view.CanSelect() {
		t.Fatal("reloaded page did not replace the previous result set")
	}
}

func TestFailedOrStaleRequestCannotChangeRetriedResults(t *testing.T) {
	view := hundredRows(t)
	old := view.Begin(100)
	if !view.Fail(old, "read_failed") || view.Apply(old, protocol.SurfacePage{State: state()}) {
		t.Fatal("a failed request became current again")
	}
	failedFirst := view.Begin(0)
	if !view.Fail(failedFirst, "read_failed") || view.Apply(failedFirst, firstPage()) {
		t.Fatal("a late first-page success revived a failed request")
	}
	latest := view.Begin(0)
	if !view.Apply(latest, firstPage()) {
		t.Fatal("retry page rejected")
	}
	view.Selected = 8
	if view.Fail(old, "late_failure") || view.Selected != 8 || len(view.Entries) != PageSize || view.Loading || view.Error != "" || !view.CanSelect() {
		t.Fatal("an old failure cleared the current retry results")
	}
}

func TestAppendingPagePreservesCurrentSelection(t *testing.T) {
	var view Model
	view.Show("session", "display", state())
	if !view.Apply(view.Begin(0), firstPage()) {
		t.Fatal("first page rejected")
	}
	view.Selected = 25
	more := view.Begin(PageSize)
	if view.Selected != 25 || view.CanSelect() {
		t.Fatal("append reset the selection or allowed a pending action")
	}
	if !view.Apply(more, protocol.SurfacePage{Entries: []protocol.EntrySummary{row(51)}, State: state()}) || view.Selected != 25 || !view.CanSelect() {
		t.Fatal("successful append lost the existing keyboard selection")
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
