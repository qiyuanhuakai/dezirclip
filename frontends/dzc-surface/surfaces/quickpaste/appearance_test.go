package quickpaste

import (
	"image/png"
	"math"
	"os"
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/theme"
)

func TestPanelVisualFixture(t *testing.T) {
	directory := os.Getenv("DZC_VISUAL_DIR")
	if directory == "" {
		t.Skip("visual export directory is not set")
	}
	if err := os.MkdirAll(directory, 0700); err != nil {
		t.Fatal(err)
	}
	for _, mode := range []string{"light", "dark"} {
		panel := &Panel{mode: mode, locale: "en", opacity: .85}
		tokens := theme.Select(mode)
		tokens.FontFamilies = []string{"Segoe UI"}
		panel.appearance = &theme.Appearance{Mode: mode, Locale: "en", Opacity: .85, Tokens: tokens}
		panel.model.Visible, panel.model.Display, panel.model.Session = true, "visual", "visual"
		panel.model.Selected = 0
		for i, value := range []string{"[image]", "https://github.com/qiyuanhuakai/dezirclip", "A rich text note with formatting and a long readable preview", "Design notes for the native clipboard panel", "A short note"} {
			value := value
			panel.model.Entries = append(panel.model.Entries, protocol.EntrySummary{Id: int64(i + 1), Kind: "text", Preview: &value, SourceApp: "DezirClip", PrivacyVisibility: "ordinary"})
		}
		frame := ui.NewTester(panel.view, 480, 360).Image()
		file, err := os.Create(filepath.Join(directory, "quickpaste-"+mode+".png"))
		if err != nil {
			t.Fatal(err)
		}
		err = png.Encode(file, frame)
		file.Close()
		if err != nil {
			t.Fatal(err)
		}
	}
}

func TestPanelPaintsSurfaceOpacityOnce(t *testing.T) {
	for _, opacity := range []float64{0, .65, 1} {
		panel := &Panel{mode: "light", locale: "en", opacity: opacity}
		panel.model.Visible = true
		frame := ui.NewTester(panel.view, 560, 440).Image()
		got := frame.RGBAAt(550, 220).A
		if math.Abs(float64(got)-opacity*255) > 2 {
			t.Fatalf("opacity %v painted alpha %d", opacity, got)
		}
		if corner := frame.RGBAAt(0, 0).A; corner != 0 {
			t.Fatalf("rounded panel left an opaque root corner: %d", corner)
		}
	}
}
