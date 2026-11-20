package main

import (
	"encoding/json"
	"strconv"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestNativeMainDrawsSavedCustomPalette(t *testing.T) {
	catalog, err := json.Marshal([]protocol.DesktopThemePackage{{
		Format: "dezirclip-theme", Version: 1, Id: "forest", Name: "Forest", Base: "mica",
		Css: `.theme-custom { --dc-surface-window: #edf3ed; --dc-text-primary: #203b2a; }
			.theme-custom.dark-mode { --dc-surface-window: #17251c; --dc-text-primary: #edf6eb; }`,
	}})
	if err != nil {
		t.Fatal(err)
	}
	window := nativeMain{mode: "light"}
	window.configureThemes("custom:forest", string(catalog))
	window.theme = "custom:forest"
	window.preferences = map[string]string{"app.custom_themes": string(catalog), "app.surface_opacity": "100"}
	tester := ui.NewTester(window.view, 360, 400)
	for _, test := range []struct {
		mode string
		rgb  [3]uint8
	}{
		{"light", [3]uint8{237, 243, 237}},
		{"dark", [3]uint8{23, 37, 28}},
	} {
		window.mode = test.mode
		tester.Frame()
		pixel := tester.Image().RGBAAt(340, 380)
		if [3]uint8{pixel.R, pixel.G, pixel.B} != test.rgb || pixel.A != 255 {
			t.Fatalf("%s shell pixel = %v, want opaque %v", test.mode, pixel, test.rgb)
		}
	}
	window.configureThemes("custom:forest", "[]")
	if window.importedTheme != nil || window.preset().Tokens().Background == "#17251c" {
		t.Fatal("removed theme remained active")
	}
}

func TestNativeMainColorMode(t *testing.T) {
	for _, systemDark := range []bool{false, true} {
		window := nativeMain{systemDark: systemDark}
		system := "light"
		if systemDark {
			system = "dark"
		}
		for preference, want := range map[string]string{"light": "light", "dark": "dark", "system": system, "": system} {
			if got := window.colorMode(map[string]string{"app.color_mode": preference}); got != want {
				t.Errorf("systemDark=%t, setting=%q: mode = %q, want %q", systemDark, preference, got, want)
			}
		}
	}
}

func TestNativeMainHeaderOrderAndSettingsClose(t *testing.T) {
	n := nativeMain{preferences: map[string]string{"app.language": "en", "app.emoji_panel_enabled": "true"}}
	view := ui.NewTester(n.view, 700, 360)
	labels := []string{"Keep on top", "Clear History", "Tags", "Emoji", "Settings", "Hide"}
	x := float32(-1)
	for _, label := range labels {
		bounds, ok := view.Find(label)
		if !ok || bounds.X <= x {
			t.Fatalf("toolbar action %q is absent or out of order: %+v", label, bounds)
		}
		x = bounds.X
	}
	if view.HasText("Add Item") {
		t.Fatal("the add-entry action replaced the clear-history action")
	}
	if err := view.Click("Settings"); err != nil {
		t.Fatal(err)
	}
	if err := view.Click("Close"); err != nil || n.showSettings {
		t.Fatalf("settings close did not return to history: %v", err)
	}
}

func TestNativeWideSettingsScroll(t *testing.T) {
	n := nativeMain{showSettings: true, settingsSection: "clipboard", preferences: map[string]string{"app.language": "en"}}
	view := ui.NewTester(n.view, 740, 310)
	view.Scroll(400, 175, 0, 280)
	if n.settingsScroll.Y <= 0 {
		t.Fatalf("wide settings did not scroll inside the detail pane: %+v", n.settingsScroll)
	}
	if nav, ok := view.Find("Privacy"); !ok || nav.X > 170 || nav.Y < 0 {
		t.Fatalf("settings navigation moved with the detail pane: %+v", nav)
	}
}

func TestNativeCompactChromeAndEmojiGrid(t *testing.T) {
	n := nativeMain{preferences: map[string]string{"app.language": "en"}}
	n.history.rows = []protocol.DesktopEntry{{Id: 1, Preview: "Short history fixture", SourceApp: "Example Editor", ContentType: "text", Tags: []string{}}}
	view := ui.NewTester(n.view, 360, 420)
	if !view.HasText("EXAMPLE EDITOR") && !view.HasText("Example Editor") {
		t.Fatal("detailed history is missing source metadata")
	}
	n.preferences["app.compact_mode"] = "true"
	view.Frame()
	if view.HasText("EXAMPLE EDITOR") || view.HasText("Example Editor") || !view.HasText("Short history fixture") {
		t.Fatal("compact mode did not replace detailed metadata")
	}
	n.showEmoji = true
	for _, width := range []int{360, 780} {
		view.SetSize(width, 420)
		emoji := emojiCatalog[0].Emojis
		first, ok := view.Find(emoji[0])
		second, ok2 := view.Find(emoji[1])
		if !ok || !ok2 || first.X > 18 || second.X <= first.X || second.Y != first.Y || second.X+second.W >= float32(width) {
			t.Fatalf("emoji grid is offset or overflowing at width %d: %+v %+v", width, first, second)
		}
	}
}

func TestNativeWideTagLibraryKeepsNavigationBesideHistory(t *testing.T) {
	n := nativeMain{tagLibrary: &nativeTagLibrary{sort: "time"}, preferences: map[string]string{"app.language": "en"}}
	n.tagCatalog.names, n.tagCatalog.counts, n.tagCatalog.fresh = []string{"Work"}, map[string]uint64{"Work": 3}, true
	view := ui.NewTester(n.view, 740, 360)
	if err := view.Click("Work (3)"); err != nil || n.tagLibrary.selected != "Work" {
		t.Fatalf("wide tag navigation did not select the tag: %v", err)
	}
	n.history.rows = []protocol.DesktopEntry{{Id: 1, Preview: "Tagged entry fixture", ContentType: "text", Tags: []string{"Work"}}}
	view.Frame()
	rail, ok := view.Find("Work (3)")
	entry, ok2 := view.Find("Tagged entry fixture")
	if !ok || !ok2 || rail.X+rail.W >= entry.X || entry.Y >= 360 {
		t.Fatalf("tag navigation and entry bounds: %+v %+v, visible texts %v", rail, entry, view.Texts())
	}
}

func TestNativeMainUsesSavedInterfaceFont(t *testing.T) {
	window := nativeMain{theme: "macos", mode: "dark", preferences: map[string]string{"app.font_main": " Custom UI "}}
	var font string
	tester := ui.NewTester(func(c *ui.Context) {
		window.view(c)
		font = c.Theme().Font
	}, 360, 400)
	if font != "Custom UI" || window.previewAppearance().theme().Font != font {
		t.Fatal("main and preview did not use the saved interface font")
	}
	delete(window.preferences, "app.font_main")
	tester.Frame()
	if font != "SF Pro Text" || window.previewAppearance().theme().Font != font {
		t.Fatal("main and preview did not restore the selected theme font")
	}
}

func TestNativeMainShellOpacity(t *testing.T) {
	for _, name := range []string{"retro", "mica", "acrylic", "liquid-glass", "macos", "scifi"} {
		customizable := name == "mica" || name == "acrylic" || name == "liquid-glass"
		for _, mode := range []string{"light", "dark"} {
			t.Run(name+"/"+mode, func(t *testing.T) {
				window := nativeMain{theme: name, mode: mode, preferences: map[string]string{"app.surface_opacity": "20", "app.language": "en"}}
				tester := ui.NewTester(window.view, 360, 400)
				want := uint8(255)
				if customizable {
					want = 51
				}
				if got := tester.Image().RGBAAt(340, 380).A; got != want {
					t.Fatalf("shell alpha = %d, want %d", got, want)
				}
				window.showSettings, window.settingsSection = true, "appearance"
				tester.Frame()
				if tester.HasText("Surface opacity") != customizable {
					t.Fatal("surface slider did not follow theme support")
				}
			})
		}
	}
}

func TestNativePreviewOpacityKeepsIndependentSetting(t *testing.T) {
	window := nativeMain{theme: "mica", mode: "dark", preferences: map[string]string{"app.surface_opacity": "20"}}
	if got := window.previewAppearance().theme().Background.A; got != 51 {
		t.Fatalf("inherited preview alpha = %d, want 51", got)
	}
	window.preferences["app.preview_opacity"] = "0"
	if got := window.previewAppearance().theme().Background.A; got != 0 {
		t.Fatalf("explicit preview alpha = %d, want 0", got)
	}
}

func TestNativeMainPrivacySettingsAreClosedOnReentry(t *testing.T) {
	for _, width := range []int{360, 700} {
		t.Run(strconv.Itoa(width), func(t *testing.T) {
			window := nativeMain{preferences: map[string]string{"app.language": "en"}}
			tester := ui.NewTester(window.view, width, 650)
			click := func(label string) {
				t.Helper()
				if err := tester.Click(label); err != nil {
					t.Fatal(err)
				}
			}
			assertClosed := func() {
				t.Helper()
				if tester.HasText("Protect sensitive clipboard content") {
					t.Fatal("privacy settings were expanded on entry")
				}
			}
			click("Settings")
			assertClosed()
			if width < 560 {
				click("General")
			}
			click("Privacy")
			if !tester.HasText("Protect sensitive clipboard content") {
				t.Fatal("selecting Privacy did not open its settings")
			}
			click("Back")
			click("Settings")
			assertClosed()
			click("Back")

			window.showPrivacyRuleManager()
			tester.Frame()
			if !tester.HasText("Privacy rules") {
				t.Fatal("the direct privacy rules shortcut did not open Privacy")
			}
			click("Back")
			click("Settings")
			assertClosed()
		})
	}
}

func TestNativeBackupConfirmationFitsMinimumWindow(t *testing.T) {
	window := nativeMain{showSettings: true, settingsSection: "data", preferences: map[string]string{"app.language": "en"}}
	window.backup = &nativeBackupDialog{
		open: true, restoring: true, mode: "replace", path: "fixture.json",
		preview: make([]protocol.BackupPreviewRow, 10),
	}
	for i := range window.backup.preview {
		window.backup.preview[i].Content = "This is an ordinary entry from the selected clipboard backup."
	}
	tester := ui.NewTester(window.view, 250, 300)
	for _, label := range []string{"Cancel", "Import"} {
		bounds, ok := tester.Find(label)
		if !ok || bounds.H <= 0 || bounds.Y < 0 || bounds.Y+bounds.H > 300 {
			t.Errorf("%s is outside the minimum-size backup dialog: %+v", label, bounds)
		}
	}
}

func TestNativeMainNavigationAndDismissal(t *testing.T) {
	for _, theme := range []string{"retro", "mica", "acrylic", "liquid-glass", "macos", "scifi"} {
		for _, mode := range []string{"light", "dark"} {
			t.Run(theme+"/"+mode, func(t *testing.T) {
				window := nativeMain{theme: theme, mode: mode, private: true, preferences: map[string]string{"app.color_mode": mode, "app.language": "en"}}
				window.history.rows = []protocol.DesktopEntry{{Id: 1, Preview: "Fixture history entry", SourceApp: "Editor", Tags: []string{"Work"}}}
				tester := ui.NewTester(window.view, 360, 400)
				if !tester.HasText("Fixture history entry") || !tester.HasText("Private session") {
					t.Fatal("main history and privacy banner did not render")
				}
				if bounds, ok := tester.Find("Work"); !ok || bounds.W < 18 || bounds.H == 0 {
					t.Fatalf("tag was clipped: %+v", bounds)
				}
				if err := tester.Click("Settings"); err != nil {
					t.Fatal(err)
				}
				if err := tester.Click("General"); err != nil {
					t.Fatal(err)
				}
				if err := tester.Click("Appearance"); err != nil {
					t.Fatal(err)
				}
				if !tester.HasText("Color mode") || tester.HasText("Fixture history entry") {
					t.Fatal("settings navigation did not replace history")
				}
				if err := tester.Click("Back"); err != nil {
					t.Fatal(err)
				}
				if !tester.HasText("Fixture history entry") {
					t.Fatal("history did not return")
				}
				window.stopSession()
				tester.Frame()
				if tester.HasText("Fixture history entry") || tester.HasText("Private session") {
					t.Fatal("dismissal retained old private session content")
				}
			})
		}
	}
}

func TestNativeMainSearchShortcutWithoutFocusedControl(t *testing.T) {
	window := nativeMain{preferences: map[string]string{"app.show_search_box": "false", "app.language": "en"}}
	tester := ui.NewTester(window.view, 360, 400)
	if _, ok := tester.Find("Search clipboard…"); ok {
		t.Fatal("search box was initially visible")
	}
	tester.Key(ui.Ctrl, ui.KeyF)
	if _, ok := tester.Find("Search clipboard…"); !ok {
		t.Fatal("window shortcut did not reveal the search box")
	}
}

func TestNativeMainHistoryPaintsTagsAfterEntryUpdate(t *testing.T) {
	window := nativeMain{theme: "mica", mode: "light", preferences: map[string]string{"app.language": "en"}}
	row := protocol.DesktopEntry{Id: 1, SourceApp: "dzc", Preview: "Fixture entry", Tags: []string{}}
	window.history.rows = []protocol.DesktopEntry{row}
	tester := ui.NewTester(window.view, 750, 650)
	window.history.reset(protocol.RuntimeState{}, nativeHistoryFilter{})
	row.Tags = []string{"work"}
	window.history.rows = []protocol.DesktopEntry{row}
	window.list.Reset()
	tester.Frame()
	frame := tester.Image()
	color := nativeTagColor("work", false)
	for y := range frame.Bounds().Dy() {
		for x := range frame.Bounds().Dx() {
			at := frame.RGBAAt(x, y)
			if at.R == color.R && at.G == color.G && at.B == color.B && at.A == color.A {
				return
			}
		}
	}
	bounds, ok := tester.Find("work")
	t.Fatalf("tag color did not paint despite label=%t and bounds=%+v", ok, bounds)
}
