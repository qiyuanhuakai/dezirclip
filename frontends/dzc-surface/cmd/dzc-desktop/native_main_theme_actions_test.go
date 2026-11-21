package main

import (
	"strconv"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestNativeAppearanceGroupsThemesInBothSettingsLayouts(t *testing.T) {
	for _, width := range []int{380, 900} {
		t.Run(strconv.Itoa(width), func(t *testing.T) {
			n := nativeMain{
				showSettings: true, settingsSection: "appearance", theme: "macos", mode: "dark",
				preferences:    map[string]string{"app.language": "en"},
				importedThemes: []protocol.DesktopThemePackage{{Id: "studio", Name: "Studio theme", Base: "macos"}},
				fontChoices:    &nativeFontChoices{},
			}
			view := ui.NewTester(n.view, width, 900)
			if width < nativeSplitWidth {
				if err := view.Click(n.text("appearance_settings")); err != nil {
					t.Fatal(err)
				}
			}
			heading, ok := view.Find(n.text("visual_theme"))
			action, actionOK := view.Find(n.text("theme_import"))
			retro, retroOK := view.Find(n.text("theme_retro"))
			mica, micaOK := view.Find(n.text("theme_mica"))
			imported, importedOK := view.Find("Studio theme")
			if !ok || !actionOK || !retroOK || !micaOK || !importedOK {
				t.Fatalf("missing appearance controls: %v", view.Texts())
			}
			if heading.Y != action.Y || action.X < heading.X+heading.W/2 || action.Y >= retro.Y || retro.Y != mica.Y ||
				retro.X+retro.W > mica.X || imported.Y <= retro.Y {
				t.Fatalf("appearance controls overlap or are out of order: heading=%+v action=%+v retro=%+v mica=%+v imported=%+v",
					heading, action, retro, mica, imported)
			}
			if view.HasText(n.text("theme_export")) || view.HasText(n.text("theme_delete")) {
				t.Fatal("imported theme actions are shown with a built-in theme selected")
			}
			n.theme = "custom:studio"
			view.Frame()
			export, exportOK := view.Find(n.text("theme_export"))
			remove, removeOK := view.Find(n.text("theme_delete"))
			if !exportOK || !removeOK || export.Y <= imported.Y || remove.Y != export.Y {
				t.Fatalf("imported theme actions are missing or misplaced: export=%+v remove=%+v", export, remove)
			}
		})
	}
}
