package main

import (
	"strconv"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestNativeCaptureSettingsUseCurrentToolsAndEngineState(t *testing.T) {
	for _, width := range []int{348, 820} {
		t.Run(strconv.Itoa(width), func(t *testing.T) {
			n := nativeMain{
				showSettings: true, settingsSection: "tools", theme: "liquid-glass", mode: "dark",
				preferences: map[string]string{"app.language": "en"},
				ocrEngine:   &protocol.OcrEngineInfo{EngineName: "Example OCR", Available: true},
			}
			view := ui.NewTester(n.view, width, 900)
			if width < nativeSplitWidth {
				if err := view.Click(n.text("tools_settings")); err != nil {
					t.Fatal(err)
				}
			}
			full, haveFull := view.Find(n.text("screenshot_full_hotkey"))
			action, haveAction := view.Find(n.text("capture_region_now"))
			status := "Example OCR (" + n.text("ocr_engine_available") + ")"
			if !haveFull || !haveAction || action.Y < full.Y || !view.HasText("MyGo Native") || !view.HasText(status) {
				t.Fatalf("capture settings are incomplete or out of order: %v", view.Texts())
			}
			n.preferences["app.screenshot_enabled"] = "false"
			n.preferences["app.quick_paste_enabled"] = "false"
			n.ocrEngineFailed = true
			view.Frame()
			if view.HasText(n.text("capture_region_now")) || view.HasText("MyGo Native") ||
				view.HasText(status) || !view.HasText(n.text("ocr_engine_check_failed")) || !view.HasText(n.text("common_retry")) {
				t.Fatalf("capture settings did not follow current state: %v", view.Texts())
			}
		})
	}
}
