package main

import (
	"context"
	"encoding/json"
	"image"
	"image/color"
	"strconv"
	"testing"
	"time"

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
	view.Scroll(85, 175, 0, 540)
	if err := view.Click(n.text("about_settings")); err != nil || n.settingsSection != "about" {
		t.Fatalf("bottom rail item remained unreachable in a short window: %v", err)
	}
}

func TestNativeResponsiveSettingsFieldPlacement(t *testing.T) {
	n := nativeMain{showSettings: true, settingsSection: "general", preferences: map[string]string{"app.language": "en"}}
	view := ui.NewTester(n.view, 699, 420)
	for _, test := range []struct{ width, columns int }{{699, 1}, {700, 2}, {999, 2}, {1000, 3}} {
		view.SetSize(test.width, 420)
		first, firstOK := view.Find(n.text("autostart"))
		second, secondOK := view.Find("Start hidden")
		third, thirdOK := view.Find("Show window near the mouse")
		if !firstOK || !secondOK || !thirdOK {
			t.Fatalf("settings controls missing at width %d: %v", test.width, view.Texts())
		}
		if (second.X > first.X) != (test.columns >= 2) || (third.X > second.X) != (test.columns == 3) ||
			(second.Y == first.Y) != (test.columns >= 2) || (third.Y == first.Y) != (test.columns == 3) {
			t.Fatalf("width %d: expected %d columns, got bounds %v, %v, %v", test.width, test.columns, first, second, third)
		}
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
		emoji := emojiCatalog().Emoji[0].Entries
		first, ok := view.Find(emoji[0].Value)
		second, ok2 := view.Find(emoji[1].Value)
		if !ok || !ok2 || first.X > 18 || second.X <= first.X || second.Y != first.Y || second.X+second.W >= float32(width) {
			t.Fatalf("emoji grid is offset or overflowing at width %d: %+v %+v", width, first, second)
		}
	}
}

func TestNativeWideTagLibraryKeepsNavigationBesideHistory(t *testing.T) {
	n := nativeMain{tagLibrary: &nativeTagLibrary{sort: "time"}, preferences: map[string]string{"app.language": "en"}}
	n.tagCatalog.names, n.tagCatalog.counts, n.tagCatalog.fresh = []string{"Work"}, map[string]uint64{"Work": 3}, true
	view := ui.NewTester(n.view, 740, 360)
	if err := view.Click("Work"); err != nil || n.tagLibrary.selected != "Work" {
		t.Fatalf("wide tag navigation did not select the tag: %v", err)
	}
	n.history.rows = []protocol.DesktopEntry{{Id: 1, Preview: "Tagged entry fixture", ContentType: "text", Tags: []string{"Work"}}}
	view.Frame()
	rail, ok := view.Find("Work")
	entry, ok2 := view.Find("Tagged entry fixture")
	if !ok || !ok2 || rail.X+rail.W >= entry.X || entry.Y >= 360 {
		t.Fatalf("tag navigation and entry bounds: %+v %+v, visible texts %v", rail, entry, view.Texts())
	}
	if err := view.Click(n.text("tag_collapse_navigation")); err != nil || !n.tagLibrary.railCollapsed {
		t.Fatalf("the tag rail did not collapse: %v", err)
	}
	if err := view.Click(n.text("tag_search_label")); err != nil {
		t.Fatal(err)
	}
	view.Frame()
	view.Type("tag:work")
	if n.tagLibrary.railCollapsed || n.tagLibrary.nameQuery != "tag:work" {
		t.Fatalf("rail collapsed=%t, search query=%q, visible labels %v", n.tagLibrary.railCollapsed, n.tagLibrary.nameQuery, view.Texts())
	}
	view.SetSize(360, 420)
	if _, ok := view.Find(n.text("tags") + ": Work"); !ok || n.tagLibrary.selected != "Work" {
		t.Fatal("switching to the narrow tag picker lost the current tag")
	}
}

func TestNativeNarrowTagPickerFiltersChoices(t *testing.T) {
	n := nativeMain{tagLibrary: &nativeTagLibrary{sort: "time"}, preferences: map[string]string{"app.language": "en"}}
	n.tagCatalog.names, n.tagCatalog.counts, n.tagCatalog.fresh = []string{"Personal", "Work"}, map[string]uint64{"Personal": 1, "Work": 3}, true
	view := ui.NewTester(n.view, 360, 420)
	if err := view.Click(n.text("tags") + ": " + n.text("tags")); err != nil {
		t.Fatal(err)
	}
	if !view.Focused(n.text("tag_search_label")) {
		t.Fatal("the picker did not focus its search field")
	}
	view.Type("tag:wor")
	if _, ok := view.Find("Personal"); ok {
		t.Fatal("unrelated tag stayed visible after typing")
	}
	if _, ok := view.Find("Work"); !ok {
		t.Fatal("the filtered tag was not offered")
	}
	view.Key(0, ui.KeyEnter)
	if n.tagLibrary.selected != "Work" {
		t.Fatal("Enter did not choose the filtered tag")
	}
	n.tagLibrary.query = "fixture"
	view.Frame()
	if err := view.Click(n.text("tag_clear_search")); err != nil || n.tagLibrary.query != "" {
		t.Fatalf("the clear button did not clear the tagged-entry search: %v", err)
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
				opened := true
				window.settingsOpen = map[string]*bool{"appearance": &opened}
				for _, id := range []string{"general", "clipboard", "privacy", "applications", "tools", "data", "about"} {
					closed := false
					window.settingsOpen[id] = &closed
				}
				tester.Frame()
				if tester.HasText("Surface opacity") != customizable {
					t.Fatal("surface slider did not follow theme support")
				}
			})
		}
	}
}

func TestNativeApplicationSelectsFitTheirSettingsColumns(t *testing.T) {
	window := nativeMain{
		showSettings: true, settingsSection: "applications",
		preferences: map[string]string{"app.language": "en"},
		applications: nativeApplicationCatalog{
			loaded: true, system: map[string]string{"text": "/example/Example Editor", "rich_text": "/example/Example Writer"},
		},
	}
	view := ui.NewTester(window.view, 800, 620)
	editor, editorOK := view.Find(window.text("system_default") + " (Example Editor)")
	writer, writerOK := view.Find(window.text("system_default") + " (Example Writer)")
	if !editorOK || !writerOK || editor.X+editor.W > writer.X || editor.Y != writer.Y {
		t.Fatalf("default applications do not fit their own columns: editor=%+v writer=%+v, visible %v", editor, writer, view.Texts())
	}
}

func TestNativeSurfaceOpacityDragUpdatesThemedControls(t *testing.T) {
	window := nativeMain{theme: "liquid-glass", mode: "light", showSettings: true,
		preferences: map[string]string{"app.surface_opacity": "20", "app.language": "en"}}
	view := ui.NewTester(func(c *ui.Context) {
		kit := window.mainKit()
		kit.Apply(c)
		palette := *c.Theme()
		palette.Background = ui.Color{}
		c.SetTheme(&palette)
		window.drawSurfaceOpacity(c, window.currentSurfaceOpacity())
		kit.SettingsDataAction(c, "Material sample", "", false)
	}, 320, 180)
	label, found := view.Find("Surface opacity")
	if !found {
		t.Fatal("surface opacity slider is missing")
	}
	sample, found := view.Find("Material sample")
	if !found {
		t.Fatal("sample settings material is missing")
	}
	sampleX, sampleY := int(sample.X+5), int(sample.Y+sample.H/2)
	before := view.Image().RGBAAt(sampleX, sampleY).A
	y := label.Y + label.H + 8
	view.Press(246, y)
	view.Move(272, y)
	view.Advance(250 * time.Millisecond)
	if live := window.currentSurfaceOpacity(); live <= 20 || window.preferences["app.surface_opacity"] != "20" {
		t.Fatalf("drag has live opacity %d and stored value %s", live, window.preferences["app.surface_opacity"])
	}
	if got := view.Image().RGBAAt(sampleX, sampleY).A; got <= before {
		t.Fatalf("settings material alpha did not increase during drag: %d to %d", before, got)
	}
	view.Release(272, y)
	window.closeSettings()
	if window.surfaceOpacityDraft != nil {
		t.Fatal("closing settings retained a temporary opacity")
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
	for _, label := range []string{"Cancel", "Import Clipboard History"} {
		bounds, ok := tester.Find(label)
		if !ok || bounds.H <= 0 || bounds.Y < 0 || bounds.Y+bounds.H > 300 {
			t.Errorf("%s is outside the minimum-size backup dialog: %+v", label, bounds)
		}
	}
}

func TestNativeBackupFormatsAndReplaceConfirmation(t *testing.T) {
	window := nativeMain{showSettings: true, settingsSection: "data", preferences: map[string]string{"app.language": "en"}}
	export := &nativeBackupDialog{open: true, format: "json", path: "fixture.json"}
	window.backup = export
	tester := ui.NewTester(window.view, 560, 420)
	if err := tester.Click("Encrypted backup"); err != nil {
		t.Fatal(err)
	}
	if export.format != "encrypted" || export.path != "" || !tester.HasText("CONFIRM PASSPHRASE") {
		t.Fatal("selecting a new backup format did not reset the output path and show encryption fields")
	}
	if err := tester.Click("Cancel"); err != nil || window.backup != nil {
		t.Fatalf("cancel did not close the backup dialog: %v", err)
	}
	restore := &nativeBackupDialog{open: true, restoring: true, mode: "merge", path: "fixture.json"}
	window.backup = restore
	tester.Frame()
	if err := tester.Click(window.text("data.import.replace")); err != nil {
		t.Fatal(err)
	}
	if restore.mode != "replace" || restore.replaceConfirmed {
		t.Fatal("selecting replacement must wait for explicit confirmation")
	}
	if err := tester.Click(window.text("native_backup_replace_confirm")); err != nil {
		t.Fatal(err)
	}
	if !restore.replaceConfirmed {
		t.Fatal("checking the replacement confirmation did not update the form")
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

func TestNativeWallpaperStaysBehindHistoryAndControls(t *testing.T) {
	n := nativeMain{theme: "liquid-glass", mode: "light", preferences: map[string]string{
		"app.language": "en", "app.surface_opacity": "20", "app.custom_background_opacity": "100", backgroundSetting: "synthetic.png",
	}}
	n.history.state.ServiceInstanceId = "test-service"
	n.history.rows = []protocol.DesktopEntry{{Id: 1, ContentType: "text", Preview: "Example history", Tags: []string{}}}
	view := ui.NewTester(n.view, 360, 410)
	before := view.Image().RGBAAt(340, 390)
	raster := image.NewRGBA(image.Rect(0, 0, 2, 2))
	background := color.RGBA{R: 25, G: 85, B: 170, A: 255}
	for y := range 2 {
		for x := range 2 {
			raster.SetRGBA(x, y, background)
		}
	}
	n.background = nativeBackground{key: n.backgroundKey(n.history.state), bitmap: ui.NewBitmap(raster)}
	view.Frame()
	after := view.Image().RGBAAt(340, 390)
	if after == background || after == before || after.A != 255 {
		t.Fatalf("wallpaper was not composited under the surface: before=%v wallpaper=%v after=%v", before, background, after)
	}
	if err := view.Click("Settings"); err != nil || !n.showSettings {
		t.Fatalf("wallpaper interfered with the controls: %v", err)
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

func TestNativeScrollToTopOverlaysHistory(t *testing.T) {
	n := nativeMain{preferences: map[string]string{"app.language": "en"}}
	for i := 1; i <= 30; i++ {
		n.history.rows = append(n.history.rows, protocol.DesktopEntry{
			Id: int64(i), Preview: "Example history row " + strconv.Itoa(i),
			SourceApp: "Editor", ContentType: "text", Tags: []string{},
		})
	}
	tester := ui.NewTester(n.view, 500, 420)
	tester.Scroll(220, 250, 0, 620)
	tester.Frame()
	button, ok := tester.Find("Scroll to top")
	if !ok || button.X+button.W >= 500 || button.Y+button.H >= 420 || n.historyScroll.Y <= 0 {
		t.Fatalf("scroll control was not over the history viewport: %+v at scroll %+v", button, n.historyScroll)
	}
	if err := tester.Click("Scroll to top"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if first, _ := n.list.List.Visible(); first != 0 {
		t.Fatalf("first visible item after scrolling to top = %d", first)
	}
}

func TestNativeCompactHistoryToolbarOpensTagsFromTheHoveredCard(t *testing.T) {
	n := nativeMain{
		theme: "macos", mode: "dark", preferences: map[string]string{"app.language": "en", "app.compact_mode": "true"},
		session: context.Background(),
	}
	n.history.state.ServiceInstanceId = "test-service"
	n.history.rows = []protocol.DesktopEntry{
		{Id: 1, ContentType: "text", Preview: "Example text", Tags: []string{}},
		{Id: 2, ContentType: "files", Preview: "Example.pdf", Tags: []string{}},
	}
	view := ui.NewTester(n.view, 440, 400)
	textCard, ok := view.Find("Example text")
	if !ok {
		t.Fatal("text card did not render")
	}
	if _, ok := view.Find("Edit tags"); ok {
		t.Fatal("a toolbar was visible before hovering")
	}
	view.Move(textCard.X+20, textCard.Y+textCard.H/2)
	view.Advance(250 * time.Millisecond)
	previous := float32(0)
	for _, label := range []string{"Edit Item Content", "Open", "Pin entry", "Edit tags", "Delete entry"} {
		control, ok := view.Find(label)
		if !ok || control.X <= previous || control.Y >= textCard.Y+textCard.H/2 {
			t.Fatalf("unexpected position for %q: %+v in card %+v, found=%t", label, control, textCard, ok)
		}
		previous = control.X
	}
	if after, _ := view.Find("Example text"); after != textCard {
		t.Fatalf("showing the toolbar changed the card bounds: %+v -> %+v", textCard, after)
	}
	fileCard, ok := view.Find("Example.pdf")
	if !ok {
		t.Fatal("attachment card did not render")
	}
	view.Move(fileCard.X+20, fileCard.Y+fileCard.H/2)
	view.Advance(250 * time.Millisecond)
	if err := view.Click("Edit tags"); err != nil {
		t.Fatal(err)
	}
	if n.tags == nil || n.tags.entry != 2 || !n.tags.open {
		t.Fatalf("the attachment toolbar did not open its tag editor: %+v", n.tags)
	}
}
