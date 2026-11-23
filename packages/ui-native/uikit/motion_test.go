package uikit

import (
	"image/color"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
)

func TestSettingsSectionUsesThemedGapsAndEntrance(t *testing.T) {
	kit := Kit{Mode: "light", widgets: map[string]theme.WidgetStyle{
		"settingsGroup":           {Opacity: 1, MarginBottom: 12},
		"settingsGroupOpen":       {Opacity: 1, MarginBottom: 12},
		"settingsGroupHeader":     {Opacity: 1, Height: 36},
		"settingsGroupHeaderOpen": {Opacity: 1, Height: 36},
		"settingsGroupContent": {
			Opacity: 1,
			Enter:   &theme.WidgetEntrance{DurationMs: 180, Y: -2, ScaleY: 1},
		},
	}}
	open, second := false, false
	accent := ui.RGB(228, 25, 129)
	tester := ui.NewTester(func(c *ui.Context) {
		SettingsSectionStack(c, func() {
			kit.SettingsSectionCard(c, "General", &open, func() {
				ui.Box(c).FillWidth().Height(25).Label("Settings content").Background(accent)
			})
			kit.SettingsSectionCard(c, "Clipboard", &second, func() {})
		})
	}, 240, 180)
	first, ok := tester.Find("General")
	next, found := tester.Find("Clipboard")
	if !ok || !found || next.Y-(first.Y+first.H) != 16 {
		t.Fatalf("expected the stack gap and theme margin between the cards, got %v and %v", first, next)
	}
	if err := tester.Click("General"); err != nil {
		t.Fatal(err)
	}
	content, ok := tester.Find("Settings content")
	if !ok {
		t.Fatal("opening a section did not mount the body")
	}
	sample := func() color.RGBA {
		return tester.Image().RGBAAt(int(content.X+content.W/2), int(content.Y+12))
	}
	solid := color.RGBA{R: accent.R, G: accent.G, B: accent.B, A: 255}
	firstFrame := sample()
	tester.Advance(45 * time.Millisecond)
	fading := sample()
	tester.Advance(180 * time.Millisecond)
	if firstFrame == solid || fading == firstFrame || fading == solid || sample() != solid {
		t.Fatalf("settings content did not fade in: initial %v, during %v, final %v", firstFrame, fading, sample())
	}
}

func TestHistoryCardInterpolatesHoverAndSelection(t *testing.T) {
	kit := Kit{Mode: "light"}
	selected := false
	view := ui.NewTester(func(c *ui.Context) {
		kit.Apply(c)
		kit.HistoryCard(c, HistoryCardOptions{Summary: "History fixture", Selected: selected})
	}, 280, 165)
	bounds, ok := view.Find("History fixture")
	if !ok {
		t.Fatal("history card did not render")
	}
	x, y := int(bounds.X+bounds.W-12), int(bounds.Y+bounds.H/2)
	pixel := func() ui.Color {
		p := view.Image().RGBAAt(x, y)
		return ui.Color{R: p.R, G: p.G, B: p.B, A: p.A}
	}
	duration := time.Duration(kit.palette().TransitionMs) * time.Millisecond
	rest := pixel()
	view.Move(float32(x), float32(y))
	view.Advance(duration / 3)
	hoverFrame := pixel()
	view.Advance(duration)
	hover := pixel()
	if rest == hover || hoverFrame == rest || hoverFrame == hover {
		t.Fatalf("hover did not interpolate: rest=%v intermediate=%v hover=%v", rest, hoverFrame, hover)
	}
	selected = true
	view.Frame()
	view.Advance(duration)
	if pixel() == hover {
		t.Fatal("selected card retained the hover appearance")
	}
}

func TestClickableComponentsUseHandCursor(t *testing.T) {
	kit := Kit{Mode: "light"}
	open, on := false, false
	selected := "work"
	for _, test := range []struct {
		label  string
		cursor ui.Cursor
		draw   func(*ui.Context)
	}{
		{"Settings button", ui.CursorPointer, func(c *ui.Context) { kit.IconButton(c, "settings", "Settings button", false, false) }},
		{"General settings", ui.CursorPointer, func(c *ui.Context) {
			kit.SettingsSectionCard(c, "General settings", &open, func() {})
		}},
		{"Automatic updates", ui.CursorPointer, func(c *ui.Context) { kit.SettingsToggle(c, &on, "Automatic updates") }},
		{"Smile", ui.CursorPointer, func(c *ui.Context) { kit.characterTile(c, CharacterChoice{Value: "Smile"}, 80, false, nil) }},
		{"History fixture", ui.CursorPointer, func(c *ui.Context) { kit.HistoryCard(c, HistoryCardOptions{Summary: "History fixture"}) }},
		{"Pin", ui.CursorPointer, func(c *ui.Context) { kit.HistoryCardActions(c, []HeaderAction{{Icon: "pin", Label: "Pin"}}, false) }},
		{"Check for updates", ui.CursorPointer, func(c *ui.Context) { kit.UpdateAction(c, UpdateCheck, "Check for updates", false) }},
		{"Update unavailable", ui.CursorDefault, func(c *ui.Context) { kit.UpdateAction(c, UpdateInstall, "Update unavailable", true) }},
		{"Remove shortcut", ui.CursorPointer, func(c *ui.Context) { kit.HotkeyRemoveButton(c, "Remove shortcut", false) }},
		{"Work", ui.CursorPointer, func(c *ui.Context) { kit.TagRailChoice(c, TagChoice{ID: "work", Label: "Work"}, true, false) }},
		{"Interface font", ui.CursorPointer, func(c *ui.Context) {
			kit.SearchSelect(c, SearchSelectOptions{
				Value: &selected, Label: "Interface font", Choices: []SelectOption{{Value: "work", Label: "Work"}},
			})
		}},
		{"Search history", ui.CursorText, func(c *ui.Context) { kit.QuickPasteSearch(c, &selected, "Search history") }},
	} {
		t.Run(test.label, func(t *testing.T) {
			view := ui.NewTester(test.draw, 280, 130)
			bounds, ok := view.Find(test.label)
			if !ok {
				t.Fatalf("missing control %q", test.label)
			}
			view.Move(bounds.X+bounds.W/2, bounds.Y+bounds.H/2)
			if cursor := view.Cursor(); cursor != test.cursor {
				t.Fatalf("hover cursor = %v, want %v", cursor, test.cursor)
			}
		})
	}
}

func TestHistoryCardAnimatesOCRExpansionAndDropsClosedTextImmediately(t *testing.T) {
	kit := Kit{Mode: "light"}
	const recognized = "Recognized clipboard text"
	open := false
	view := ui.NewTester(func(c *ui.Context) {
		kit.Apply(c)
		kit.HistoryCard(c, HistoryCardOptions{
			Summary: "Image card",
			Body:    func() { ui.Box(c).Height(20) },
			OCRResult: &HistoryOCRResultOptions{
				Label: "OCR", ToggleLabel: "Toggle OCR", Text: recognized, Expanded: open,
				OnToggle: func() { open = !open },
			},
		})
	}, 280, 180)
	height := func() float32 {
		t.Helper()
		bounds, ok := view.Find("Image card")
		if !ok {
			t.Fatal("missing image card")
		}
		return bounds.H
	}
	duration := time.Duration(kit.palette().TransitionMs) * time.Millisecond
	collapsed := height()
	open = true
	view.Frame()
	view.Advance(duration / 3)
	animating := height()
	view.Advance(duration)
	expanded := height()
	if !(collapsed < animating && animating < expanded) {
		t.Fatalf("opening heights %v, %v, %v did not animate", collapsed, animating, expanded)
	}
	if !view.HasText(recognized) {
		t.Fatal("expanded card did not expose its OCR result")
	}
	open = false
	view.Frame()
	if view.HasText(recognized) {
		t.Fatal("collapsed card retained its OCR result during the transition")
	}
	view.Advance(duration / 3)
	animating = height()
	view.Advance(duration)
	if !(collapsed < animating && animating < expanded) || height() != collapsed {
		t.Fatalf("closing heights %v, %v, %v did not animate back to %v", expanded, animating, height(), collapsed)
	}
}
