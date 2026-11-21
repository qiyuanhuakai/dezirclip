package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestSearchSelectFiltersByPathAndCommitsOnlyAListedValue(t *testing.T) {
	value := ""
	view := ui.NewTester(func(c *ui.Context) {
		Kit{}.SearchSelect(c, SearchSelectOptions{
			Label: "Images", NoMatches: "No matching applications", Value: &value,
			Choices: []SelectOption{
				{Label: "System default"},
				{Value: "/first/viewer", Label: "Image Viewer"},
				{Value: "/second/viewer", Label: "Image Viewer"},
			},
		})
	}, 360, 300)
	if err := view.Click("Images"); err != nil {
		t.Fatal(err)
	}
	view.Type("second viewer")
	if value != "" {
		t.Fatal("filtering changed the saved choice")
	}
	if err := view.Click("Image Viewer"); err != nil {
		t.Fatal(err)
	}
	if value != "/second/viewer" {
		t.Fatalf("selected %q, expected the matched application path", value)
	}
	if err := view.Click("Images"); err != nil {
		t.Fatal(err)
	}
	view.Type("zzzz000")
	if _, ok := view.Find("No matching applications"); !ok {
		t.Fatal("expected an explicit empty state for the search")
	}
	view.Key(0, ui.KeyEnter)
	if value != "/second/viewer" {
		t.Fatal("no-results search discarded the committed choice")
	}
	view.Key(0, ui.KeyEscape)
	if err := view.Click("Images"); err != nil {
		t.Fatal(err)
	}
	view.Type("first veiwer")
	if _, ok := view.Find("Image Viewer"); !ok {
		t.Fatal("a transposed letter hid the matching application")
	}
	view.Key(0, ui.KeyEnter)
	if value != "/first/viewer" {
		t.Fatalf("Enter committed %q instead of the matching application's path", value)
	}
	if err := view.Click("Images"); err != nil {
		t.Fatal(err)
	}
	if err := view.Click("System default"); err != nil {
		t.Fatal(err)
	}
	if value != "" {
		t.Fatal("choosing the system default did not clear the path")
	}
}
