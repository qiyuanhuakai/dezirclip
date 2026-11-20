package uikit

import (
	"fmt"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestCharacterListScrollsToLastGroupWithoutPaginationButton(t *testing.T) {
	var state ui.ListState
	selected := ""
	choices := make([]CharacterChoice, 300)
	for i := range choices {
		choices[i] = CharacterChoice{Value: fmt.Sprintf("%03d", i)}
	}
	last := "✓"
	firstGroup, lastGroup := "FIRST GROUP", "LAST GROUP"
	groups := []CharacterGroup{{Label: firstGroup, Choices: choices}, {Label: lastGroup, Choices: []CharacterChoice{{Value: last}}}}
	view := ui.NewTester(func(c *ui.Context) {
		ui.Column(c).Fill().Children(func() {
			Kit{}.CharacterList(c, &state, 260, groups, "Symbols", "No results", false, func(value string) {
				selected = value
			})
		})
	}, 260, 165)
	if !view.HasText(firstGroup) || view.HasText(lastGroup) || len(view.Texts()) >= len(choices) {
		t.Fatalf("first viewport exposed %d labels, first group %t, last group %t",
			len(view.Texts()), view.HasText(firstGroup), view.HasText(lastGroup))
	}
	for range 5 {
		view.Scroll(130, 90, 0, 3200)
		if view.HasText(lastGroup) && view.HasText(last) {
			break
		}
	}
	if !view.HasText(lastGroup) {
		t.Fatalf("the last group was unreachable by scrolling: %v", view.Texts())
	}
	if err := view.Click(last); err != nil || selected != last {
		t.Fatalf("last glyph selected = %q, %v", selected, err)
	}
}
