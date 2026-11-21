package uikit

import (
	"math"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestEmojiFavoritesAcceptsFilesAndCentersAShortRow(t *testing.T) {
	const width = 360
	var list ui.ListState
	var names []string
	var dropped []string
	var dragging bool
	added := 0
	kit := Kit{}
	view := ui.NewTester(func(c *ui.Context) {
		kit.EmojiFavorites(c, &list, EmojiFavoritesOptions{
			ContentWidth: width, Count: len(names), Label: "Favorites", AddLabel: "Add emoji",
			EmptyTitle: "No emoji yet", Hint: "Choose or drop images", DropLabel: "Drop images here", Dragging: dragging,
			OnAdd:  func() { added++ },
			OnDrop: func(paths []string) { dropped = slices.Clone(paths) },
			Item: func(c *ui.Context, index int, width float32) {
				kit.EmojiFavoriteTile(c, EmojiFavoriteOptions{
					Name: names[index], Width: width, Preview: func() {},
				})
			},
		})
	}, width, 240)

	add, ok := view.Find("Add emoji")
	if !ok || !view.HasText("No emoji yet") || !view.HasText("Choose or drop images") {
		t.Fatalf("empty favorites view: %v", view.Texts())
	}
	center := func(x float32) bool { return math.Abs(float64(x-width/2)) <= 3 }
	if !center(add.X + add.W/2) {
		t.Fatalf("the add tile is off center on its own: %+v", add)
	}
	if err := view.Click("Add emoji"); err != nil || added != 1 {
		t.Fatalf("add action: %v, count=%d", err, added)
	}
	dragging = true
	view.Frame()
	if !view.HasText("Drop images here") || view.HasText("No emoji yet") {
		t.Fatalf("drop hint and empty-state message overlapped: %v", view.Texts())
	}
	dragging = false
	view.Frame()
	paths := []string{"first.png", "second.gif"}
	if !view.DropFiles(width/2, 186, paths...) || !slices.Equal(dropped, paths) {
		t.Fatalf("drop did not reach the gallery: %v", dropped)
	}

	names = []string{"First image", "Second image"}
	view.Frame()
	add, addOK := view.Find("Add emoji")
	last, lastOK := view.Find("Second image")
	if !addOK || !lastOK || !center((add.X+last.X+last.W)/2) || view.HasText("No emoji yet") {
		t.Fatalf("populated gallery is not centered: add=%+v last=%+v, visible=%v", add, last, view.Texts())
	}
}
