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
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

func TestNativeHistoryUsesResponsiveAttachmentGallery(t *testing.T) {
	window := nativeMain{theme: "macos", mode: "light", preferences: map[string]string{"app.language": "en"}}
	window.history.rows = []protocol.DesktopEntry{
		{Id: 1, Timestamp: 1, SourceApp: "Fixture", ContentType: "files", Preview: "Test attachment group", Tags: []string{}},
	}
	bitmap := ui.NewBitmap(image.NewRGBA(image.Rect(0, 0, 160, 96)))
	window.cardMedia.finish(nativeCardKey{1, 1}, nativeCardMedia{attachments: []uikit.PreviewAttachment{
		{Name: "First.png", Bitmap: bitmap}, {Name: "Second.png", Bitmap: bitmap},
		{Name: "Notes.pdf"}, {Name: "Archive.zip"}, {Name: "Planning.xlsx"},
	}})
	tester := ui.NewTester(window.view, 350, 420)
	for _, label := range []string{"First.png", "Second.png", "Notes.pdf", "…"} {
		if bounds, ok := tester.Find(label); !ok || bounds.H <= 0 {
			t.Errorf("wide gallery is missing %s", label)
		}
	}
	tester.SetSize(240, 420)
	if _, ok := tester.Find("Second.png"); ok {
		t.Fatal("the narrow history card kept a second ordinary-mode thumbnail")
	}
	window.preferences["app.compact_mode"] = "true"
	tester.Frame()
	if _, ok := tester.Find("Second.png"); !ok {
		t.Fatal("the compact gallery did not use the narrower thumbnail columns")
	}
	window.history.rows[0].Tags = nil
	tester.Frame()
	if _, ok := tester.Find("First.png"); ok {
		t.Fatal("the protected history card displayed a cached attachment")
	}
}

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

func TestNativeClipboardSettingsKeepSequentialControlsTogether(t *testing.T) {
	n := nativeMain{showSettings: true, settingsSection: "clipboard", preferences: map[string]string{"app.language": "en"}}
	view := ui.NewTester(n.view, 670, 760)
	shortcut := n.text("sequential_paste_hotkey_label")
	clear := n.text("sequential_paste_clear_queue")
	if view.HasText(shortcut) || view.HasText(clear) {
		t.Fatal("disabled sequential paste showed its shortcut or queue controls")
	}
	n.preferences["app.sequential_mode"] = "true"
	n.queueCount.known = true
	view.Frame()
	if !view.HasText(shortcut) || view.HasText(clear) {
		t.Fatal("empty sequential queue should show only the shortcut")
	}
	n.queueCount.count = 3
	view.Frame()
	order := []string{n.text("search_hotkey_label"), "Move pasted entries to top", n.text("paste_method"),
		n.text("sequential_paste_mode"), clear, shortcut, n.text("global_hotkey")}
	lastY := float32(-1)
	for _, label := range order {
		bounds, shown := view.Find(label)
		if !shown || bounds.Y <= lastY {
			t.Fatalf("%q missing or out of order: %v shown=%t", label, bounds, shown)
		}
		lastY = bounds.Y
	}
}

func TestNativeResponsiveSettingsFieldPlacement(t *testing.T) {
	n := nativeMain{showSettings: true, settingsSection: "general", preferences: map[string]string{"app.language": "en"}}
	view := ui.NewTester(n.view, 699, 420)
	for _, test := range []struct{ width, columns int }{{699, 1}, {700, 2}, {999, 2}, {1000, 3}} {
		view.SetSize(test.width, 420)
		first, firstOK := view.Find(n.text("autostart"))
		second, secondOK := view.Find(n.text("hide_tray_icon"))
		third, thirdOK := view.Find(n.text("edge_docking"))
		if !firstOK || !secondOK || !thirdOK {
			t.Fatalf("settings controls missing at width %d: %v", test.width, view.Texts())
		}
		sameRow := func(y float32) bool { return y >= first.Y-2 && y <= first.Y+2 }
		if (second.X > first.X) != (test.columns >= 2) || (third.X > second.X) != (test.columns == 3) ||
			sameRow(second.Y) != (test.columns >= 2) || sameRow(third.Y) != (test.columns == 3) {
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

func TestNativeHistoryRendersCachedRichTextOnlyForVisibleContent(t *testing.T) {
	window := nativeMain{preferences: map[string]string{"app.language": "en"}}
	const richText = "<p><strong>Rich clipboard sample</strong></p><p>Details from the original entry</p>"
	row := protocol.DesktopEntry{Id: 1, Timestamp: 1, ContentType: "rich_text", SourceApp: "Editor",
		Preview: "Short clipboard summary", Tags: []string{}}
	window.history.rows = []protocol.DesktopEntry{row}
	key := nativeCardKey{row.Id, row.Timestamp}
	window.cardMedia.finish(key, nativeCardMedia{rich: uikit.ParseHistoryRichText(richText)})
	view := ui.NewTester(window.view, 400, 350)
	if !view.HasText("Details from the original entry") {
		t.Fatalf("card did not draw its cached formatting: %v", view.Texts())
	}
	window.history.rows[0].Tags = nil
	view.Frame()
	hiddenLabel := nativeCardSummary(window.history.rows[0], window.language())
	if view.HasText("Details from the original entry") || !view.HasText(hiddenLabel) {
		t.Fatalf("card exposed rich content after concealment: %v", view.Texts())
	}
	authorized := row
	markup := "<p>Details from the authorized reveal</p>"
	authorized.HtmlContent = &markup
	window.protectedRows = map[nativeCardKey]*nativeProtectedRow{
		key: {cancel: func() {}, body: newProtectedBody(authorized, window.language())},
	}
	view.Frame()
	if !view.HasText("Details from the authorized reveal") || view.HasText("Details from the original entry") {
		t.Fatalf("revealed card did not use its authorized body: %v", view.Texts())
	}
	window.clearProtectedRows()
	view.Frame()
	if view.HasText("Details from the authorized reveal") {
		t.Fatal("card retained revealed content after it was hidden again")
	}
}

func TestNativeEmojiAutomaticallyReachesFinalChoices(t *testing.T) {
	window := nativeMain{showEmoji: true, preferences: map[string]string{"app.language": "en"}}
	view := ui.NewTester(window.view, 360, 330)
	groups := window.emojiPanel().matches
	lastGroup := groups[len(groups)-1]
	symbol := lastGroup.choices[len(lastGroup.choices)-1].Value
	if view.HasText(symbol) {
		t.Fatal("initial viewport rendered the last emoji")
	}
	for attempt := 0; attempt < 12; attempt++ {
		view.Scroll(180, 235, 0, 3200)
		bounds, rendered := view.Find(symbol)
		if rendered && bounds.Y >= 0 && bounds.Y+bounds.H <= 330 {
			return
		}
	}
	t.Fatalf("scrolling alone did not reach the last emoji %q", symbol)
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
				window := nativeMain{theme: name, mode: mode, preferences: map[string]string{"app.language": "en"}}
				tester := ui.NewTester(window.view, 360, 400)
				for _, percent := range []int{0, 20, 65, 85, 100} {
					window.preferences["app.surface_opacity"] = strconv.Itoa(percent)
					tester.Frame()
					want := 255
					if customizable {
						want = (percent*255 + 50) / 100
					}
					if name == "liquid-glass" {
						light := map[int]int{0: 0, 20: 95, 65: 228, 85: 252, 100: 255}
						dark := map[int]int{0: 0, 20: 100, 65: 235, 85: 255, 100: 255}
						want = light[percent]
						if mode == "dark" {
							want = dark[percent]
						}
					}
					if got := int(tester.Image().RGBAAt(340, 380).A); got < want-1 || got > want+1 {
						t.Fatalf("at %d%% painted panel alpha = %d, want %d", percent, got, want)
					}
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

func TestNativeFontSelectionsStayVisibleBeforeAndAfterDiscovery(t *testing.T) {
	window := nativeMain{
		theme: "macos", mode: "dark", readingFonts: true,
		preferences: map[string]string{
			"app.language": "en", "app.font_main": "Saved Interface", "app.font_mono": "Saved Monospace",
		},
	}
	view := ui.NewTester(window.typographySettings, 365, 380)
	verify := func() {
		t.Helper()
		if !view.HasText("Saved Interface") || !view.HasText("Saved Monospace") {
			t.Fatalf("the selected fonts were hidden: %v", view.Texts())
		}
	}
	verify()
	window.readingFonts, window.fontsFailed = false, true
	view.Frame()
	verify()
	window.fontsFailed = false
	window.fontChoices = &nativeFontChoices{main: []string{"Installed Sans"}, mono: []string{"Installed Mono"}}
	view.Frame()
	verify()
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

func TestNativeUpdateChannelHelpFollowsTheSelectedChannel(t *testing.T) {
	window := nativeMain{preferences: map[string]string{"app.language": "en"}}
	window.updater.channel = "preview"
	view := ui.NewTester(func(c *ui.Context) {
		window.updateSettings(c, window.mainKit())
	}, 460, 340)
	label := window.settingsHelpLabel(window.text("updater_channel"))
	if err := view.Click(label); err != nil || !view.HasText(window.text("updater_preview_hint")) {
		t.Fatalf("preview channel help did not open: %v, visible %v", err, view.Texts())
	}
	window.updater.channel = "stable"
	view.Frame()
	if !view.HasText(window.text("updater_stable_hint")) || view.HasText(window.text("updater_preview_hint")) {
		t.Fatalf("channel help retained the previous value: %v", view.Texts())
	}
}

func TestNativeSettingsFieldHelpExpandsIndependently(t *testing.T) {
	window := nativeMain{showSettings: true, settingsSection: "appearance",
		preferences: map[string]string{"app.language": "en"},
		fontChoices: &nativeFontChoices{main: []string{"Example Sans"}, mono: []string{"Example Mono"}}}
	view := ui.NewTester(window.view, 850, 700)
	label := window.settingsHelpLabel(window.text("font_main", "Interface font"))
	hint := window.text("font_main_hint")
	if view.HasText(hint) {
		t.Fatal("font help was expanded when settings opened")
	}
	if err := view.Click(label); err != nil || !view.HasText(hint) {
		t.Fatalf("font help did not open: %v, visible: %v", err, view.Texts())
	}
	if view.HasText(window.text("font_mono_hint")) {
		t.Fatal("opening interface font help also opened monospace font help")
	}
	if err := view.Click(label); err != nil || view.HasText(hint) {
		t.Fatalf("font help did not close: %v", err)
	}
}

func TestNativeGeneralSettingsOrderAndKeyboardPreviewDependency(t *testing.T) {
	n := &nativeMain{preferences: map[string]string{
		"app.language": "en", "app.idle_destroy_enabled": "true", "app.sound_enabled": "true",
	}}
	view := ui.NewTester(n.generalSettings, 390, 810)
	labels := []string{"autostart", "hide_tray_icon", "edge_docking", "follow_mouse", "idle_destroy_enabled", "idle_destroy_seconds",
		"sound_effects", "sound_volume", "paste_sound", "silent_start", "show_search_box", "scroll_top_button",
		"emoji_panel_enabled", "tag_manager_enabled", "arrow_key_selection", "preview_keyboard_enabled"}
	previousY := float32(-1)
	for _, label := range labels {
		box, found := view.Find(n.text(label))
		if !found || box.Y <= previousY {
			t.Fatalf("%s missing or out of order at %+v after y=%.1f", label, box, previousY)
		}
		previousY = box.Y
	}
	arrowLabel := n.text("arrow_key_selection")
	previewLabel := n.text("preview_keyboard_enabled")
	arrow, _ := view.Find(arrowLabel)
	preview, _ := view.Find(previewLabel)
	if preview.X-arrow.X < 17 {
		t.Fatalf("preview should be nested under arrow navigation: arrow=%+v preview=%+v", arrow, preview)
	}
	view.ClickAt(365, preview.Y+preview.H/2)
	if view.Focused(previewLabel) {
		t.Fatal("keyboard preview should be disabled when arrow navigation is off")
	}
	if err := view.Click(n.settingsHelpLabel(previewLabel)); err != nil || !view.HasText(n.text("preview_keyboard_enabled_hint")) {
		t.Fatalf("disabled preview should retain its help: %v", err)
	}
	if err := view.Click(n.settingsHelpLabel(previewLabel)); err != nil {
		t.Fatal(err)
	}
	n.preferences["app.arrow_key_selection"] = "true"
	view.Frame()
	preview, _ = view.Find(previewLabel)
	view.ClickAt(365, preview.Y+preview.H/2)
	if !view.Focused(previewLabel) {
		t.Fatal("keyboard preview should accept interaction when arrow navigation is on")
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
				if tester.HasText(window.text("privacy_protection")) {
					t.Fatal("privacy settings were expanded on entry")
				}
			}
			click("Settings")
			assertClosed()
			click("Privacy")
			if !tester.HasText(window.text("privacy_protection")) {
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

func TestNativeAboutShowsTheUpdateCandidateInConfirmation(t *testing.T) {
	window := nativeMain{showSettings: true, settingsSection: "about", session: context.Background(),
		preferences: map[string]string{"app.language": "en"}}
	window.history.state.ServiceInstanceId = "fixture"
	window.updater.channel = "preview"
	if err := json.Unmarshal([]byte(`{"channel":"preview","status":"available","available":{"version":"2.0.0","artifact":{"url":"package","signature":"signature"}}}`), &window.updater.result); err != nil {
		t.Fatal(err)
	}
	window.updater.result.installTarget, window.updater.result.installSupport = "fixture-installer", "updater_support_nsis"
	view := ui.NewTester(window.view, 760, 690)
	support := window.text("updater_support_nsis")
	if view.HasText(support) {
		t.Fatal("installation support help should start closed")
	}
	if err := view.Click(window.settingsHelpLabel(window.text("updater_title"))); err != nil || !view.HasText(support) {
		t.Fatalf("installation support help did not open: %v", err)
	}
	title := window.text("updater_confirm_title") + " v2.0.0"
	if err := view.Click(window.text("updater_install")); err != nil || !view.HasText(title) {
		t.Fatalf("install confirmation should name the chosen release: %v", err)
	}
	window.updater.applyCheck(&nativeUpdateCheck{Result: nativeUpdateResult{Channel: "preview", Status: "current"}, Checked: time.Now()})
	view.Frame()
	if window.updater.confirm || view.HasText(title) {
		t.Fatal("confirmation stayed open after the release became unavailable")
	}
}

func TestNativeUpdateProgressSurvivesClosingSettings(t *testing.T) {
	window := nativeMain{showSettings: true, settingsSection: "about", preferences: map[string]string{"app.language": "en"}}
	window.updater.applyInstallStatus(residentUpdateStatus{
		Progress: nativeInstallProgress{Phase: updateDownloading, Downloaded: 42, Total: 100},
	})
	tester := ui.NewTester(window.view, 640, 540)
	if !tester.HasText("42%") {
		t.Fatal("update download progress is missing from About")
	}
	if err := tester.Click(window.text("common_close")); err != nil {
		t.Fatal(err)
	}
	if window.showSettings || !window.updater.installing() || window.updater.keepWindow() || !tester.HasText("42%") {
		t.Fatal("closing settings hid or cancelled the download")
	}
	window.updater.applyInstallStatus(residentUpdateStatus{Progress: nativeInstallProgress{Phase: updateInstalling}})
	tester.Frame()
	if !tester.HasText(window.text("updater_installing")) || !tester.HasText(window.text("updater_verified")) {
		t.Fatal("install handoff is missing from the main view")
	}
	window.updater.applyInstallStatus(residentUpdateStatus{Failure: errUpdateVerification.Error()})
	tester.Frame()
	if !tester.HasText(window.text("updater_error_download_verify")) {
		t.Fatal("the failure was hidden after settings were closed")
	}
}

func TestNativeHistoryCardsResizeWithClipboardFont(t *testing.T) {
	const plain = "Fixture history text"
	const spaced = "Text with \r\n\t repeated  spacing"
	window := nativeMain{theme: "macos", mode: "light", preferences: map[string]string{"app.language": "en"}}
	window.history.rows = []protocol.DesktopEntry{
		{Id: 1, SourceApp: "Editor", ContentType: "text", Preview: plain, Tags: []string{}},
		{Id: 2, SourceApp: "Editor", ContentType: "text", Preview: spaced, Tags: []string{}},
	}
	for _, compact := range []string{"false", "true"} {
		window.preferences["app.compact_mode"] = compact
		window.preferences["app.clipboard_item_font_size"] = "11"
		tester := ui.NewTester(window.view, 360, 400)
		small, found := tester.Find(plain)
		if !found || !tester.HasText("Text with repeated spacing") {
			t.Fatal("history preview did not render using normalized display spacing")
		}
		window.preferences["app.clipboard_item_font_size"] = "18"
		tester.Frame()
		large, found := tester.Find(plain)
		if !found || large.H-small.H < 7 {
			t.Fatalf("compact=%s: 11px card height %.1f, 18px card height %.1f", compact, small.H, large.H)
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
