package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"image/png"
	"log"
	"os"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-presets"
)

type gallery struct {
	theme, mode, fixture, state       string
	choice, query, searchMode, filter string
	settingsSelection                 string
	emojiFavorites, settingEnabled    bool
	historyList                       uikit.HistoryListState
	historySelected                   int
	previewImage                      uikit.PreviewImage
	previewOpacity                    float64
	previewPinned                     bool
	wheelCount                        int
	resizeDirection                   uikit.PreviewResizeDirection
	dialog, disclosure, help, visible bool
	focusTagName                      bool
	active                            map[string]bool
}

func (g *gallery) view(c *ui.Context) {
	preset := presets.Select(g.theme, g.mode)
	kit := preset.Kit()
	kit.Apply(c)
	padding := preset.ShowcasePadding()
	ui.Column(c).Fill().Padding(padding[:]...).Gap(14).Background(c.Theme().Background).Children(func() {
		if g.fixture == "gallery" {
			for _, name := range []string{"select", "search-bar", "filters", "highlight", "search-history", "disclosure", "help"} {
				g.renderFixture(c, kit, name)
			}
			return
		}
		g.renderFixture(c, kit, g.fixture)
	})
}

func (g *gallery) renderFixture(c *ui.Context, kit uikit.Kit, name string) {
	switch name {
	case "attachment-preview":
		g.attachmentGallery(c, kit)
	case "preview-toolbar":
		g.previewToolbar(c, kit)
	case "preview-image":
		g.imageViewport(c)
	case "preview-resize-handles", "wheel-hot-zone":
		g.inputRegion(c, name == "wheel-hot-zone")
	case "history-card-meta":
		kit.HistoryCard(c, uikit.HistoryCardOptions{
			Source: "Editor", Time: "12:30", Pinned: true, Body: func() {},
			Actions: []uikit.HeaderAction{{Icon: "tag", Label: "Tags"}},
		})
	case "history-list":
		g.renderHistoryList(c, kit)
	case "settings-rail":
		if g.settingsSelection == "" {
			g.settingsSelection = "appearance"
			if g.state == "selected" {
				g.settingsSelection = "updates"
			}
		}
		kit.SettingsRail(c, "Settings", g.settingsSelection, []uikit.SettingsSection{
			{ID: "appearance", Label: "Appearance"}, {ID: "general", Label: "General"}, {ID: "updates", Label: "Software updates"},
		}, func(section string) { g.settingsSelection = section })
	case "settings-footer":
		kit.SettingsFooter(c, uikit.SettingsFooterOptions{
			ProductName: "DezirClip 0.2", Feedback: "Feedback", Reset: "Reset defaults", Website: "Official website",
			Compact: g.state == "compact",
		})
	case "settings-toggle":
		kit.SettingsToggle(c, &g.settingEnabled, "Keep clipboard history")
	case "settings-slider":
		ui.Column(c).FillWidth().Gap(6).Children(func() {
			ui.Text(c, fmt.Sprintf("Opacity %.0f%%", g.previewOpacity))
			kit.Slider(c, &g.previewOpacity, 0, 100, "Opacity").FillWidth().Step(1)
		})
	case "hotkey-binding":
		ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Text(c, "Quick Paste").Grow(1)
			kit.HotkeyBinding(c, "Quick Paste", "Ctrl+Shift+V", "Set shortcut", g.state == "recording", false)
		})
	case "scroll-to-top":
		ui.Column(c).FillWidth().Height(120).Children(func() {
			ui.Text(c, "Clipboard history").Grow(1)
			ui.Row(c).FillWidth().Justify(ui.End).Children(func() {
				kit.ScrollToTopButton(c, "Scroll to top")
			})
		})
	case "emoji-tabs":
		kit.EmojiTabs(c, "Emoji", "Favorites", g.emojiFavorites, false, func(favorites bool) { g.emojiFavorites = favorites })
	case "emoji-grid":
		kit.EmojiGrid(c, "Frequently used", []string{"😀", "😄", "😂", "🤣", "😊", "😍", "😘", "😎", "🤔", "🔥", "✨", "👌"}, false, nil)
	case "app-header":
		pinned := g.state == "pinned"
		pin := uikit.HeaderAction{Icon: "pin", Label: "Pin", Active: pinned, Run: func() {
			if pinned {
				g.state = "default"
			} else {
				g.state = "pinned"
			}
		}}
		if pinned {
			pin.Icon = "unpin"
		}
		options := uikit.HeaderOptions{Title: "DezirClip", Query: &g.query, Placeholder: "Search clipboard...",
			Filters: []uikit.HeaderFilter{{ID: "text", Label: "Text"}, {ID: "image", Label: "Image"}, {ID: "file", Label: "File"}},
			Filter:  &g.filter, Actions: []uikit.HeaderAction{pin}}
		if g.state == "settings" {
			options.Title, options.Query = "Settings", nil
			options.Back = &uikit.HeaderAction{Icon: "back", Label: "Back", Run: func() { g.state = "default" }}
		} else {
			options.Actions = append(options.Actions,
				uikit.HeaderAction{Icon: "clear", Label: "Clear History"},
				uikit.HeaderAction{Icon: "tag", Label: "Tag Manager"},
				uikit.HeaderAction{Icon: "emoji", Label: "Emoji"},
				uikit.HeaderAction{Icon: "settings", Label: "Settings", Run: func() { g.state = "settings" }})
		}
		options.Actions = append(options.Actions, uikit.HeaderAction{Icon: "hide", Label: "Hide"})
		kit.Header(c, options)
	case "history-card":
		kit.HistoryCard(c, uikit.HistoryCardOptions{Source: "Editor", Time: "12:30",
			Summary: "Copied from the active editor", Selected: g.state == "selected", Pinned: g.state == "pinned",
			Actions: []uikit.HeaderAction{{Icon: "pin", Label: "Pin"}, {Icon: "tag", Label: "Tags"}, {Icon: "hide", Label: "Delete"}}})
	case "select":
		kit.Select(c, &g.choice, "Color mode", []uikit.SelectOption{
			{Value: "system", Label: "System"},
			{Value: "light", Label: "Light"},
			{Value: "dark", Label: "Dark"},
			{Value: "disabled", Label: "Unavailable", Disabled: true},
		}, g.state == "disabled")
	case "search-bar":
		results := 0
		if g.query != "" {
			results = 1
		}
		message := ""
		if g.state == "error" {
			message = "Search unavailable"
		}
		kit.SearchBar(c, uikit.SearchOptions{Query: &g.query, Mode: &g.searchMode,
			Modes: []uikit.SearchMode{{ID: "fts", Label: "FTS5"}, {ID: "fuzzy", Label: "模糊"}, {ID: "regex", Label: "正则"}},
			Label: "Search", Placeholder: "搜索剪贴板内容...", ClearLabel: "清除", Error: message, Results: results, Loading: g.state == "loading"})
	case "filters":
		kit.Filters(c, []uikit.FilterOption{
			{ID: "text", Label: "Text", Count: 7}, {ID: "image", Label: "Image", Count: 2}, {ID: "file", Label: "File", Count: 1},
		}, g.active, "All", "Clear")
	case "highlight":
		kit.MatchHighlight(c, "Clipboard history", "history")
	case "search-history":
		var entries []string
		if g.state != "empty" {
			entries = []string{"Clipboard", "Documents"}
		}
		kit.SearchHistory(c, entries, "No recent searches", func(query string) { g.query = query })
	case "disclosure":
		kit.SettingsDisclosure(c, "Software updates", &g.disclosure, func() { ui.Text(c, "Preview channel").FontSize(16) })
	case "help":
		kit.SettingHelp(c, "Setting help: Software updates", &g.help, "Updates are checked when the app starts.")
	case "progress":
		if g.visible {
			kit.ProgressToast(c, "Importing", 0.6, g.state, func() { g.visible = false })
		}
	case "toast":
		kit.Toast(c, "Copied to clipboard")
	case "confirm":
		kit.ConfirmDialog(c, &g.dialog, "Clear history", "Clear all unpinned entries?", "Cancel", "Clear", func() {})
	case "tag-dialog":
		kit.Dialog(c, &g.dialog, "Create tag", "Close", g.state == "busy", func() {
			field := kit.TagNameField(c, &g.query, "Name", g.state == "busy")
			if g.focusTagName {
				field.Focus()
				g.focusTagName = false
			}
		})
	default:
		ui.Text(c, "Unknown component: "+name)
	}
}

func (g *gallery) prepare(tester *ui.Tester) error {
	switch g.state {
	case "southeast", "scrolled":
		bounds, ok := tester.Find("Input target")
		if !ok {
			return fmt.Errorf("input target is absent")
		}
		if g.state == "southeast" {
			tester.Press(bounds.X+bounds.W-4, bounds.Y+bounds.H-4)
			tester.Release(bounds.X+bounds.W-4, bounds.Y+bounds.H-4)
		} else {
			tester.Move(bounds.X+20, bounds.Y+20)
			tester.Wheel(bounds.X+20, bounds.Y+20, 0, 52)
		}
	case "expanded":
		if g.fixture == "disclosure" {
			return tester.Click("Software updates")
		}
		if g.fixture == "help" {
			return tester.Click("Setting help: Software updates")
		}
	case "hover":
		bounds, ok := tester.Find("System")
		if !ok {
			return fmt.Errorf("select trigger is absent")
		}
		tester.Move(bounds.X+bounds.W/2, bounds.Y+bounds.H/2)
	case "focus":
		tester.Key(0, ui.KeyTab)
	case "popup":
		return tester.Click("System")
	}
	return nil
}

func run() error {
	g := gallery{choice: "system", searchMode: "fts", dialog: true, visible: true, previewOpacity: 72, active: map[string]bool{"text": true}}
	flag.StringVar(&g.theme, "theme", "mica", "built-in theme")
	flag.StringVar(&g.mode, "mode", "light", "color mode")
	flag.StringVar(&g.fixture, "fixture", "gallery", "component to render")
	flag.StringVar(&g.state, "state", "default", "component state")
	output := flag.String("output", "", "write a software-rendered PNG to this file")
	inspect := flag.Bool("inspect", false, "write rendered text labels as JSON")
	flag.Parse()
	g.settingEnabled = g.state == "enabled"
	g.emojiFavorites = g.state == "favorites"
	g.historySelected = -1
	if g.fixture == "history-list" && g.state == "selected" {
		g.historySelected = 1
	}
	if g.fixture == "preview-toolbar" {
		g.previewPinned = g.state == "pinned"
	}
	if g.fixture == "preview-image" && g.state == "zoomed" {
		g.previewImage.SetZoom(2)
	}
	if g.state == "filled" {
		g.query = "Clipboard"
	}
	if g.fixture == "tag-dialog" {
		g.query = "Work"
		g.focusTagName = true
	}
	const width, height = 560, 420
	if *output != "" {
		tester := ui.NewTester(g.view, width, height)
		tester.SetDark(g.mode == "dark")
		if err := g.prepare(tester); err != nil {
			return err
		}
		transition := presets.Select(g.theme, g.mode).Tokens().TransitionMs
		tester.Advance(time.Duration(transition) * time.Millisecond)
		if *inspect {
			if err := json.NewEncoder(os.Stdout).Encode(tester.Texts()); err != nil {
				return err
			}
		}
		file, err := os.Create(*output)
		if err != nil {
			return err
		}
		encodeErr := png.Encode(file, tester.Image())
		closeErr := file.Close()
		if encodeErr != nil {
			return fmt.Errorf("write gallery image: %w", encodeErr)
		}
		return closeErr
	}
	mygo.App.WhenReady(func() {
		mygo.NewWindow(mygo.WindowOptions{Title: "Deziro UI Components", Width: width, Height: height, Content: ui.View(g.view)})
	})
	return mygo.App.Run()
}

func main() {
	if err := run(); err != nil {
		log.Fatal(err)
	}
}
