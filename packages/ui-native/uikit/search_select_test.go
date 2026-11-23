package uikit

import (
	"fmt"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestSearchSelectKeepsLongOptionsInsideItsFieldWidth(t *testing.T) {
	const long = "Example Windows application with a very long display name that should wrap inside the settings column"
	var selected string
	options := []SelectOption{{Value: "long", Label: long}}
	for i := range 30 {
		options = append(options, SelectOption{Value: fmt.Sprint(i), Label: fmt.Sprintf("Application %02d", i)})
	}
	kit := Kit{}
	view := ui.NewTester(func(c *ui.Context) {
		ui.Column(c).Width(215).Margin(140, 0, 0, 170).Children(func() {
			kit.SearchSelect(c, SearchSelectOptions{Label: "Default application", Value: &selected, Choices: options})
		})
	}, 495, 350)
	anchor, ok := view.Find("Default application")
	if !ok {
		t.Fatal("missing search field")
	}
	if err := view.Click("Default application"); err != nil {
		t.Fatal(err)
	}
	row, ok := view.Find(long)
	if !ok || row.X < anchor.X-1 || row.X+row.W > anchor.X+anchor.W+1 || row.W < anchor.W-14 {
		t.Fatalf("option expanded the menu past its anchor: option=%+v anchor=%+v", row, anchor)
	}
	if row.Y < 0 || row.Y+row.H > 350 || row.Y < anchor.Y+anchor.H && row.Y+row.H > anchor.Y {
		t.Fatalf("menu overlapped the field or ran out of the window: option=%+v anchor=%+v", row, anchor)
	}
	if err := view.Click(long); err != nil || selected != "long" {
		t.Fatalf("could not choose the wrapped option: selected=%q error=%v", selected, err)
	}
}

func TestSettingsRailWrapsLongSectionNames(t *testing.T) {
	const label = "Default Applications and File Associations"
	var selected string
	view := ui.NewTester(func(c *ui.Context) {
		Kit{}.SettingsRail(c, "Settings", selected, []SettingsSection{
			{ID: "general", Label: "General"}, {ID: "applications", Label: label},
		}, func(id string) { selected = id })
	}, 360, 250)
	short, shortOK := view.Find("General")
	long, longOK := view.Find(label)
	if !shortOK || !longOK || long.H <= short.H {
		t.Fatalf("long section name did not wrap: short=%+v long=%+v", short, long)
	}
	if err := view.Click(label); err != nil || selected != "applications" {
		t.Fatalf("wrapped label was not clickable: selected=%q error=%v", selected, err)
	}
}

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
	field, ok := view.Find("Images")
	if !ok {
		t.Fatal("search field disappeared when opened")
	}
	view.Move(field.X+field.W/2, field.Y+field.H/2)
	if cursor := view.Cursor(); cursor != ui.CursorText {
		t.Fatalf("open search field cursor = %v, want text cursor", cursor)
	}
	choice, ok := view.Find("Image Viewer")
	if !ok {
		t.Fatal("search options did not open")
	}
	view.Move(choice.X+choice.W/2, choice.Y+choice.H/2)
	if cursor := view.Cursor(); cursor != ui.CursorPointer {
		t.Fatalf("option cursor = %v, want hand cursor", cursor)
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

func TestSearchSelectOpensFromTheKeyboardAndShowsTheCommittedChoice(t *testing.T) {
	selected := "font-27"
	var options []SelectOption
	for i := range 30 {
		options = append(options, SelectOption{Value: fmt.Sprintf("font-%d", i), Label: fmt.Sprintf("Example family %02d", i)})
	}
	view := ui.NewTester(func(c *ui.Context) {
		ui.Column(c).Width(220).Children(func() {
			Kit{}.SearchSelect(c, SearchSelectOptions{Label: "Interface font", Value: &selected, Choices: options})
		})
	}, 340, 300)
	view.Key(0, ui.KeyTab)
	if !view.Focused("Interface font") {
		t.Fatal("the font selector did not receive keyboard focus")
	}
	view.Key(0, ui.KeyEnter)
	anchor, _ := view.Find("Interface font")
	// The previous family only appears in the popup, near the choice shown in the field.
	row, ok := view.Find("Example family 26")
	if !ok || row.H <= 0 || row.Y < anchor.Y+anchor.H || row.Y+row.H > 300 {
		t.Fatalf("the current font was not visible in the opened list: %+v, found=%t", row, ok)
	}
	view.Key(0, ui.KeyEnter)
	if selected != "font-27" {
		t.Fatalf("closing the open list changed the committed choice to %q", selected)
	}
	if _, ok := view.Find("Example family 26"); ok {
		t.Fatal("confirming the displayed choice should close the font list")
	}
}

func TestSearchSelectMatchesAccentsWithoutChangingTheStoredPath(t *testing.T) {
	choices := []SelectOption{
		{Value: "/apps/other/editor", Label: "Other editor"},
		{Value: "/apps/Café/Éditeur", Label: "Café Éditeur"},
	}
	if got := filterSearchChoices(choices, "Cafe editeur"); len(got) != 1 || got[0] != choices[1] {
		t.Fatalf("accented application search = %#v", got)
	}
}
