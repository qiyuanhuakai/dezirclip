package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"image/png"
	"log"
	"os"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/theme"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type gallery struct {
	theme, mode, fixture, state       string
	choice, query, searchMode, filter string
	settingsSelection                 string
	dialog, disclosure, help, visible bool
	active                            map[string]bool
}

func (g *gallery) view(c *ui.Context) {
	uikit.Kit{Name: g.theme, Mode: g.mode}.Apply(c)
	padding := theme.BuiltinWidgetStyle(g.theme, g.mode, "gallery").Padding
	ui.Column(c).Fill().Padding(padding[:]...).Gap(14).Background(c.Theme().Background).Children(func() {
		if g.fixture == "gallery" {
			for _, name := range []string{"select", "search-bar", "filters", "highlight", "search-history", "disclosure", "help"} {
				g.renderFixture(c, name)
			}
			return
		}
		g.renderFixture(c, g.fixture)
	})
}

func (g *gallery) renderFixture(c *ui.Context, name string) {
	kit := uikit.Kit{Name: g.theme, Mode: g.mode}
	switch name {
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
			Modes: []uikit.SearchMode{{ID: "fts", Label: "FTS5"}, {ID: "fuzzy", Label: "≈ 模糊"}, {ID: "regex", Label: ".* 正则"}},
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
			ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
				ui.Text(c, "Name")
				uikit.SearchField(c, &g.query, "Name", "")
			})
		})
	default:
		ui.Text(c, "Unknown component: "+name)
	}
}

func (g *gallery) prepare(tester *ui.Tester) error {
	switch g.state {
	case "expanded":
		if g.fixture == "disclosure" {
			return tester.Click("Software updates")
		}
		return tester.Click("Setting help: Software updates")
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
	g := gallery{choice: "system", searchMode: "fts", dialog: true, visible: true, active: map[string]bool{"text": true}}
	flag.StringVar(&g.theme, "theme", "mica", "built-in theme")
	flag.StringVar(&g.mode, "mode", "light", "color mode")
	flag.StringVar(&g.fixture, "fixture", "gallery", "component to render")
	flag.StringVar(&g.state, "state", "default", "component state")
	output := flag.String("output", "", "write a software-rendered PNG to this file")
	inspect := flag.Bool("inspect", false, "write rendered text labels as JSON")
	flag.Parse()
	if g.state == "filled" {
		g.query = "Clipboard"
	}
	if g.fixture == "tag-dialog" {
		g.query = "Work"
	}
	const width, height = 560, 420
	if *output != "" {
		tester := ui.NewTester(g.view, width, height)
		tester.SetDark(g.mode == "dark")
		if err := g.prepare(tester); err != nil {
			return err
		}
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
