package uikit

import (
	"fmt"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestHistoryListBuildsOnlyVisibleRowsAndRequestsEachPageOnce(t *testing.T) {
	kit := Kit{}
	state := &HistoryListState{}
	selected, count, requests := -1, 300, 0
	built := map[int]bool{}
	state.List.Selected = &selected
	state.List.Key = func(index int) any { return index + 1 }
	view := ui.NewTester(func(c *ui.Context) {
		kit.Apply(c)
		ui.Column(c).Fill().Children(func() {
			kit.HistoryList(c, state, HistoryListOptions{
				Count: count, HasMore: true, MoreLabel: "Load more",
				OnLoadMore: func() { requests++ },
				Row: func(row ui.ListRow) {
					built[row.Index] = true
					kit.HistoryCard(row.Context, HistoryCardOptions{
						Summary: fmt.Sprintf("Clipboard entry %d", row.Index), Selected: row.Selected(),
					})
				},
			})
		})
	}, 420, 300)
	if len(built) == 0 || len(built) > 20 || requests != 0 {
		t.Fatalf("opening list built %d rows and requested %d pages", len(built), requests)
	}
	state.List.ScrollToEnd()
	view.Frame()
	view.Frame()
	view.Frame()
	if requests != 1 || !built[count-1] {
		t.Fatalf("reaching end requested %d pages, last row built %v", requests, built[count-1])
	}
	count += 40
	state.List.ScrollToEnd()
	view.Frame()
	view.Frame()
	if requests != 2 || !built[count-1] {
		t.Fatalf("reaching next page requested %d pages, last row built %v", requests, built[count-1])
	}
	state.Reset()
	view.Frame()
	first, _ := state.List.Visible()
	if first != 0 {
		t.Fatalf("reset left first visible row at %d", first)
	}
}
