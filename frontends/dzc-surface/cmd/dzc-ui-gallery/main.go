package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"image/png"
	"log"
	"os"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-presets"
)

type gallery struct {
	characterList                       ui.ListState
	theme, mode, fixture, state         string
	choice, query, searchMode, filter   string
	settingsSelection                   string
	emojiFavorites, settingEnabled      bool
	historyList                         uikit.HistoryListState
	historySelected                     int
	previewImage                        uikit.PreviewImage
	previewOpacity                      float64
	previewPinned                       bool
	wheelCount                          int
	resizeDirection                     uikit.PreviewResizeDirection
	dialog, disclosure, help, visible   bool
	focusTagName                        bool
	active                              map[string]bool
	contextMenu                         uikit.ContextMenu
	entryName, searchTerms, idle, notes string
	groupOpen                           bool
	colorChoice, channelChoice          string
	bindings                            []string
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
	if g.parityFixture(c, kit, name) {
		return
	}
	switch name {
	case "text-fields":
		labeled := func(label string, field func()) {
			ui.Column(c).Gap(3).Children(func() {
				kit.SettingsLabel(c, label)
				field()
			})
		}
		ui.Column(c).Gap(8).Children(func() {
			labeled("Entry name", func() { kit.TextField(c, &g.entryName, "Clipboard entry name").Width(330) })
			labeled("Search terms", func() { kit.TextField(c, &g.searchTerms, "Search entries").Placeholder("Filter by name").Width(330) })
			labeled("Idle timeout", func() { kit.NumberField(c, &g.idle, "Seconds").Width(112) })
			labeled("Notes", func() { kit.TextArea(c, &g.notes, "Entry notes").Width(330).Height(60) })
		})
	case "context-menu":
		anchor := ui.ButtonBase(c).Label("Clipboard entry").Width(185).Height(32)
		anchor.Children(func() { ui.Text(c, "Clipboard entry").FontSize(14).SingleLine().PassThrough() })
		if anchor.RightClicked() {
			g.contextMenu.Open(anchor, func() []uikit.MenuAction {
				return []uikit.MenuAction{
					{Label: "Copy"},
					{Label: "Text processing", Children: []uikit.MenuAction{{Label: "Uppercase"}, {Label: "Lowercase"}, {Label: "Trim spaces"}}},
					{Label: "Privacy", Children: []uikit.MenuAction{{Label: "Ignore this source"}, {Label: "Protect this source"}, {Label: "Manage rules"}}},
					{Label: "Delete"},
				}
			})
		}
		kit.ContextMenu(c, &g.contextMenu, "Close menu")
	case "attachment-preview":
		g.attachmentGallery(c, kit)
	case "history-image":
		g.historyImages(c, kit)
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
		ui.Column(c).FillWidth().Padding(6, 0).Children(func() {
			kit.SettingsLabel(c, fmt.Sprintf("Opacity %.0f%%", g.previewOpacity))
			kit.Slider(c, &g.previewOpacity, 0, 100, "Opacity").FillWidth().Step(1)
		})
	case "hotkey-binding":
		kit.HotkeyRow(c, "Quick Paste", func() {
			kit.HotkeyBinding(c, "Quick Paste", "Ctrl+Shift+V", "Press a shortcut", g.state == "recording", false)
		})
	case "scroll-to-top":
		ui.Column(c).FillWidth().Height(136).Children(func() {
			ui.Text(c, "Clipboard history").FontSize(16)
			kit.ScrollToTopButton(c, "Scroll to top").Absolute().Right(16).Bottom(16)
		})
	case "character-search":
		kit.CharacterSearch(c, &g.query, "Search symbols", nil)
	case "emoji-tabs":
		kit.EmojiTabs(c, "Emoji", "Favorites", g.emojiFavorites, false, func(favorites bool) { g.emojiFavorites = favorites })
	case "emoji-grid":
		width, _ := c.Size()
		padding := presets.Select(g.theme, g.mode).ShowcasePadding()
		kit.CharacterList(c, &g.characterList, width-padding[1]-padding[3],
			[]uikit.CharacterGroup{{Label: "Frequently used", Choices: []uikit.CharacterChoice{
				{Value: "😀"}, {Value: "😄"}, {Value: "😂"}, {Value: "🤣"}, {Value: "😊"}, {Value: "😍"},
				{Value: "😘"}, {Value: "😎"}, {Value: "🤔"}, {Value: "🔥"}, {Value: "✨"}, {Value: "👌"},
			}}}, "Emoji", "No results", false, nil)
	case "clipboard-search":
		kit.ClipboardSearch(c, uikit.HeaderOptions{
			Query: &g.query, Placeholder: "搜索剪切板...", Filter: &g.filter,
			Filters: []uikit.HeaderFilter{
				{ID: "text", Label: "文本"}, {ID: "image", Label: "图片"}, {ID: "file", Label: "文件"}, {ID: "url", Label: "URL"},
				{ID: "code", Label: "代码"}, {ID: "video", Label: "视频"}, {ID: "rich_text", Label: "富文本"},
			},
		})
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
		active := strings.Contains(g.state, "active")
		kit.HistoryCard(c, uikit.HistoryCardOptions{Source: "Editor", Time: "12:30",
			Summary: "Copied from the active editor", Selected: g.state == "selected",
			Pinned: g.state == "pinned", Compact: strings.HasPrefix(g.state, "compact"),
			Actions: []uikit.HeaderAction{{Icon: "pin", Label: "Pin", Active: active}, {Icon: "tag", Label: "Tags"}, {Icon: "hide", Label: "Delete"}}})
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
	if g.fixture == "emoji-favorites" {
		if err := tester.RightClick("Example image"); err != nil {
			return err
		}
		tester.Move(450, 390)
		return nil
	}
	if g.fixture == "context-menu" {
		tester.RightClickAt(72, 43)
		if g.state != "default" {
			label := "Privacy"
			if g.state == "text" {
				label = "Text processing"
			}
			previous, _ := tester.Find("Delete")
			bounds, ok := tester.Find(label)
			if !ok {
				return fmt.Errorf("submenu trigger absent: %s", label)
			}
			tester.Move(bounds.X+bounds.W/2, bounds.Y+bounds.H/2)
			if later, _ := tester.Find("Delete"); previous != later {
				return fmt.Errorf("opening a submenu moved the following root item")
			}
		}
		return nil
	}
	if g.fixture == "text-fields" && g.state == "focus-text" {
		return tester.Click("Clipboard entry name")
	}
	if g.fixture == "settings-slider" && g.state == "keyboard" {
		tester.Key(0, ui.KeyTab)
		tester.Key(0, ui.KeyRight)
		if g.previewOpacity != 73 {
			return fmt.Errorf("slider keyboard step = %.0f, expected 73", g.previewOpacity)
		}
		return nil
	}
	if g.fixture == "clipboard-search" && g.state == "selected" {
		if err := tester.Click("图片"); err != nil {
			return err
		}
		if g.filter != "image" {
			return fmt.Errorf("image filter did not become selected")
		}
		return nil
	}
	if g.fixture == "search-select" && g.state != "default" {
		if err := tester.Click("Images"); err != nil {
			return err
		}
		if g.state == "empty" {
			tester.Type("zzzz0000")
			if _, found := tester.Find("No matching applications"); !found {
				return fmt.Errorf("no-results notice did not appear")
			}
		} else {
			tester.Type("viewer")
			if _, found := tester.Find("Photo Viewer"); !found {
				return fmt.Errorf("filtered application did not appear")
			}
		}
		return nil
	}
	switch g.state {
	case "compact", "compact-hover", "compact-active-hover", "compact-action-hover":
		if g.fixture != "history-card" {
			break
		}
		if _, ok := tester.Find("Pin"); ok {
			return fmt.Errorf("compact toolbar was present before hovering")
		}
		if g.state == "compact" {
			break
		}
		bounds, ok := tester.Find("Copied from the active editor")
		if !ok {
			return fmt.Errorf("history card is absent")
		}
		tester.Move(bounds.X+40, bounds.Y+bounds.H/2)
		tester.Advance(time.Duration(presets.Select(g.theme, g.mode).Tokens().TransitionMs) * time.Millisecond)
		pin, ok := tester.Find("Pin")
		if !ok {
			return fmt.Errorf("compact toolbar was absent after hovering")
		}
		if g.state != "compact-hover" {
			tester.Move(pin.X+pin.W/2, pin.Y+pin.H/2)
		}
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
		label := "System"
		if g.fixture == "scroll-to-top" {
			label = "Scroll to top"
		}
		bounds, ok := tester.Find(label)
		if !ok {
			return fmt.Errorf("hover target %q is absent", label)
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
	g.entryName, g.idle, g.notes = "Release checklist", "30", "First line\nSecond line"
	flag.StringVar(&g.theme, "theme", "mica", "built-in theme")
	flag.StringVar(&g.mode, "mode", "light", "color mode")
	flag.StringVar(&g.fixture, "fixture", "gallery", "component to render")
	flag.StringVar(&g.state, "state", "default", "component state")
	output := flag.String("output", "", "write a software-rendered PNG to this file")
	inspect := flag.Bool("inspect", false, "write rendered text labels as JSON")
	flag.Parse()
	if g.fixture == "search-select" {
		g.choice = ""
	}
	g.groupOpen = g.state == "expanded"
	g.colorChoice, g.channelChoice = "light", "preview"
	g.bindings = []string{"Super+V"}
	if g.state == "multiple" || g.state == "win-v" {
		g.bindings = append(g.bindings, "Ctrl+Shift+V")
	}
	if g.state == "extended" {
		g.bindings = []string{"Ctrl+1", "Ctrl+Numpad1", "Alt+Semicolon"}
	}
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
			var result any = tester.Texts()
			if g.fixture == "history-typography" {
				cards := make(map[string]ui.Rect, len(historyTypographySamples))
				for _, sample := range historyTypographySamples {
					bounds, ok := tester.Find(sample.name)
					if !ok {
						return fmt.Errorf("no history card named %s", sample.name)
					}
					cards[sample.name] = bounds
				}
				result = struct {
					Texts []string
					Cards map[string]ui.Rect
				}{tester.Texts(), cards}
			} else if g.fixture == "history-image" || g.fixture == "attachment-preview" {
				result = inspectMediaCards(g.fixture, tester)
			}
			if err := json.NewEncoder(os.Stdout).Encode(result); err != nil {
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
