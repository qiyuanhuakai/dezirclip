package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestTextFieldsInsetTypedAndExistingValues(t *testing.T) {
	number, setting, note := "30", "Clipboard history", "First line"
	view := ui.NewTester(func(c *ui.Context) {
		kit := Kit{}
		ui.Column(c).Gap(6).Children(func() {
			kit.TextField(c, &setting, "Name").Width(200)
			kit.NumberField(c, &number, "Timeout").Width(90)
			kit.TextArea(c, &note, "Notes").Width(200).Height(70)
		})
	}, 240, 180)
	for _, label := range []string{"Name", "Timeout", "Notes"} {
		field, ok := view.Find(label)
		if !ok {
			t.Fatalf("missing field %s", label)
		}
		if err := view.Click(label); err != nil {
			t.Fatal(err)
		}
		view.Key(0, ui.KeyHome)
		if caret, ok := view.TextCaret(); !ok || caret.X < field.X+8 || caret.X >= field.X+field.W-8 {
			t.Fatalf("%s text begins against a field edge: caret=%+v field=%+v, found=%t", label, caret, field, ok)
		}
	}
	view.Key(0, ui.KeyEnd)
	view.Type(" added")
	if note != "First line added" || number != "30" || setting != "Clipboard history" {
		t.Fatal("editing one inset field did not preserve the other values")
	}
}
