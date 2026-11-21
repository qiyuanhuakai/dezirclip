package main

import (
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

func symbolChoices(first rune, count int) []uikit.CharacterChoice {
	options := make([]uikit.CharacterChoice, count)
	for index := range options {
		options[index].Value = string(first + rune(index))
	}
	return options
}

func (g *gallery) characterPanelFixture(c *ui.Context, kit uikit.Kit) {
	width, _ := c.Size()
	groups := []uikit.CharacterGroup{
		{Label: "Arrows", Choices: symbolChoices(0x2190, 112)},
		{Label: "Technical symbols", Choices: symbolChoices(0x2300, 104)},
		{Label: "Geometric shapes", Choices: symbolChoices(0x25a0, 80)},
		{Label: "Mathematics", Choices: []uikit.CharacterChoice{
			{Value: "∑"}, {Value: "∞"}, {Value: "√"}, {Value: "∫"}, {Value: "≈"}, {Value: "≠"}, {Value: "≤"}, {Value: "≥"},
		}},
	}
	ui.Column(c).FillWidth().Height(300).Children(func() {
		kit.CharacterPanel(c, width, func(contentWidth float32) {
			kit.CharacterTabs(c, []uikit.CharacterTab{
				{ID: "emoji", Label: "Emoji"}, {ID: "symbols", Label: "Symbols"}, {ID: "favorites", Label: "Favorites"},
			}, "symbols", false, func(string) {})
			kit.CharacterSearch(c, &g.query, "Search symbols", nil)
			kit.CharacterList(c, &g.characterList, contentWidth, groups, "Symbols", "No results", false, nil)
		})
	})
}
