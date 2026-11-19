package main

import (
	"context"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestTagSuggestionsPreserveSelectionAcrossCatalogRefresh(t *testing.T) {
	catalog := nativeTagCatalog{revision: 1, names: []string{"misc", "work", "Work Notes", "Workspace"}}
	var picker nativeTagSuggestions
	picker.refresh("  TAG: work ", &catalog)
	if !slices.Equal(picker.matches, []string{"work", "Work Notes", "Workspace"}) || picker.active != -1 {
		t.Fatalf("search matched %v and selected %d", picker.matches, picker.active)
	}
	if !picker.move(ui.KeyDown) || !picker.move(ui.KeyDown) || picker.active != 1 {
		t.Fatal("Arrow Down did not select the second matching tag")
	}
	catalog.revision++
	catalog.names = []string{"Work Notes", "Workspace"}
	picker.refresh("  TAG: work ", &catalog)
	if picker.active != 0 || picker.matches[picker.active] != "Work Notes" {
		t.Fatal("refresh changed the highlighted tag")
	}
	picker.refresh("tag:space", &catalog)
	if picker.active != -1 || !slices.Equal(picker.matches, []string{"Workspace"}) {
		t.Fatal("a different query kept the former highlight")
	}
}

func TestNativeSearchTagPopupSelectsUsingKeyboard(t *testing.T) {
	n := nativeMain{
		session:     context.Background(),
		query:       "tag:wor",
		preferences: map[string]string{"app.language": "en"},
		tagCatalog:  nativeTagCatalog{revision: 1, fresh: true, names: []string{"other", "Work Notes", "Workspace"}},
	}
	n.history.state = protocol.RuntimeState{ServiceInstanceId: "fixture"}
	tester := ui.NewTester(func(c *ui.Context) {
		input := ui.TextInput(c, &n.query).Width(240).Label("Search clipboard").OnTextKey(n.searchKey)
		n.tagSuggestionsView(c, input)
	}, 260, 310)
	if err := tester.Click("Search clipboard"); err != nil {
		t.Fatal(err)
	}
	if !tester.HasText("Work Notes") || tester.HasText("other") {
		t.Fatal("opening the search field did not show the filtered tag choices")
	}
	tester.Key(0, ui.KeyDown)
	tester.Key(0, ui.KeyEnter)
	if n.query != "tag:Work Notes" || n.tagSuggestions.open {
		t.Fatalf("query = %q, popup open = %t", n.query, n.tagSuggestions.open)
	}
}
