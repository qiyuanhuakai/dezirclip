package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestTagHistorySelectionHasOwnGutterAndTogglesOnce(t *testing.T) {
	checked, disabled := false, false
	toggles, cardActions := 0, 0
	view := ui.NewTester(func(c *ui.Context) {
		kit := Kit{Mode: "light"}
		kit.Apply(c)
		kit.TagHistoryEntry(c, HistoryCardOptions{
			Source: "Editor", Summary: "Tagged entry", Width: 240, Compact: true,
			Body: func() {
				button := ui.ButtonBase(c).Label("Embedded action").Size(30, 14)
				if button.Clicked() {
					cardActions++
				}
			},
		}, &TagHistorySelection{
			Label: "Select tagged entry", Checked: checked, Disabled: disabled,
			OnToggle: func() { checked = !checked; toggles++ },
		})
	}, 240, 120)

	checkbox, checkboxOK := view.Find("Select tagged entry")
	body, bodyOK := view.Find("Embedded action")
	if !checkboxOK || !bodyOK || checkbox.W != 15 || checkbox.H != 15 || checkbox.X+checkbox.W >= body.X {
		t.Fatalf("selection stays outside the preview: checkbox = %v, preview = %v", checkbox, body)
	}

	view.ClickAt(checkbox.X+checkbox.W/2, checkbox.Y+checkbox.H/2)
	if !checked || toggles != 1 {
		t.Fatalf("checkbox toggled %d times and left checked = %v", toggles, checked)
	}
	view.Key(0, ui.KeyEnter)
	if checked || toggles != 2 {
		t.Fatalf("focused checkbox should toggle once on Enter, got %d, %v", toggles, checked)
	}
	view.ClickAt(body.X+body.W/2, body.Y+body.H/2)
	if !checked || toggles != 3 || cardActions != 0 {
		t.Fatalf("preview click should only toggle the row, got %d toggles, checked = %v, %d embedded actions", toggles, checked, cardActions)
	}
	view.Key(0, ui.KeySpace)
	if checked || toggles != 4 {
		t.Fatalf("focused row should toggle once on Space, got %d, %v", toggles, checked)
	}
	disabled = true
	view.Frame()
	view.ClickAt(checkbox.X+checkbox.W/2, checkbox.Y+checkbox.H/2)
	view.ClickAt(body.X+body.W/2, body.Y+body.H/2)
	if toggles != 4 {
		t.Fatalf("disabled checkbox or row accepted input: %d toggles", toggles)
	}
}
