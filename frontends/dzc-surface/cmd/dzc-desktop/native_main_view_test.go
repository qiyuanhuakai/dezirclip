package main

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestNativeMainNavigationAndDismissal(t *testing.T) {
	for _, theme := range []string{"retro", "mica", "acrylic", "liquid-glass", "macos", "scifi"} {
		for _, mode := range []string{"light", "dark"} {
			t.Run(theme+"/"+mode, func(t *testing.T) {
				window := nativeMain{theme: theme, mode: mode, private: true, preferences: map[string]string{"app.color_mode": mode}}
				window.history.rows = []protocol.DesktopEntry{{Id: 1, Preview: "Fixture history entry", SourceApp: "Editor"}}
				tester := ui.NewTester(window.view, 360, 400)
				if !tester.HasText("Fixture history entry") || !tester.HasText("Private session") {
					t.Fatal("main history and privacy banner did not render")
				}
				if err := tester.Click("Settings"); err != nil {
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
