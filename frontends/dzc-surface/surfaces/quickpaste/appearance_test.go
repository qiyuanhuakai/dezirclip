package quickpaste

import (
	"encoding/json"
	"fmt"
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

func TestPanelKeepsReadableOpacityAndTransparentCorners(t *testing.T) {
	for _, opacity := range []float64{0, .65, 1} {
		panel := &Panel{mode: "light", locale: "en", opacity: opacity}
		panel.model.Visible = true
		frame := ui.NewTester(panel.view, 560, 440).Image()
		got := frame.RGBAAt(550, 220).A
		if math.Abs(float64(got)-max(.92, opacity)*255) > 2 {
			t.Fatalf("opacity %v painted alpha %d", opacity, got)
		}
		if corner := frame.RGBAAt(0, 0).A; corner != 0 {
			t.Fatalf("rounded panel left an opaque root corner: %d", corner)
		}
	}
}

func TestPointerSelectionLeavesStationaryPointerOutOfKeyboardNavigation(t *testing.T) {
	p := &Panel{mode: "light", locale: "en", opacity: 1}
	p.model.Visible, p.model.Display, p.model.Session = true, "pointer", "pointer"
	for i, label := range []string{"First item", "Second item", "Third item"} {
		p.model.Entries = append(p.model.Entries, protocol.EntrySummary{Id: int64(i + 1), Kind: "text", Preview: &label, PrivacyVisibility: "ordinary"})
	}
	frame := ui.NewTester(p.view, 560, 440)
	r, ok := frame.Find("Second item")
	if !ok {
		t.Fatal("missing second row")
	}
	x, y := r.X+r.W/2, r.Y+r.H/2
	frame.Move(x, y)
	if p.model.Selected != 1 {
		t.Fatalf("hover selected %d", p.model.Selected)
	}
	frame.Key(0, ui.KeyDown)
	frame.Frame()
	if p.model.Selected != 2 {
		t.Fatalf("stationary pointer overrode keyboard: %d", p.model.Selected)
	}
	frame.Move(x+1, y)
	if p.model.Selected != 1 {
		t.Fatalf("pointer movement did not restore hover selection: %d", p.model.Selected)
	}
	if p.model.PendingAction != "" {
		t.Fatal("hover submitted a paste")
	}
	p.model.PendingAction = "in-flight"
	r, _ = frame.Find("First item")
	frame.Move(r.X+r.W/2, r.Y+r.H/2)
	if p.model.Selected != 1 {
		t.Fatal("pointer changed the in-flight selection")
	}
}

func TestSingleClickSubmitsOnceOnRelease(t *testing.T) {
	p := &Panel{mode: "light", locale: "en", opacity: 1, out: make(chan outgoing, 4)}
	p.model.Visible, p.model.Display, p.model.Session = true, "pointer", "pointer"
	for i, label := range []string{"First item", "Second item"} {
		p.model.Entries = append(p.model.Entries, protocol.EntrySummary{Id: int64(i + 1), Kind: "text", Preview: &label, PrivacyVisibility: "ordinary"})
	}
	t.Cleanup(func() {
		if p.actionDeadline != nil {
			p.actionDeadline.Stop()
		}
		for len(p.out) > 0 {
			(<-p.out).done <- nil
		}
	})
	frame := ui.NewTester(p.view, 560, 440)
	r, ok := frame.Find("Second item")
	if !ok {
		t.Fatal("missing second row")
	}
	x, y := r.X+r.W/2, r.Y+r.H/2
	frame.Move(x, y)
	frame.Press(x, y)
	if len(p.out) != 0 {
		t.Fatal("submitted before pointer release")
	}
	frame.Release(x, y)
	if len(p.out) != 1 {
		t.Fatalf("single click emitted %d actions", len(p.out))
	}
	action := <-p.out
	action.done <- nil
	var paste protocol.SurfacePaste
	if err := json.Unmarshal(action.message.Payload, &paste); err != nil {
		t.Fatal(err)
	}
	if action.message.Type != protocol.SurfaceControlTypePaste || paste.EntryId != 2 {
		t.Fatalf("wrong action: %+v", action.message)
	}
	frame.ClickAt(x, y)
	frame.Key(0, ui.KeyEnter)
	if len(p.out) != 0 {
		t.Fatal("replayed the pending paste")
	}
}

func TestKeyboardScrollDoesNotSelectUnderStationaryPointer(t *testing.T) {
	p := &Panel{mode: "light", locale: "en", opacity: 1}
	p.model.Visible, p.model.Display, p.model.Session = true, "scroll", "scroll"
	for i := range 30 {
		label := fmt.Sprintf("Item %02d", i)
		p.model.Entries = append(p.model.Entries, protocol.EntrySummary{Id: int64(i + 1), Kind: "text", Preview: &label, PrivacyVisibility: "ordinary"})
	}
	frame := ui.NewTester(p.view, 560, 440)
	r, ok := frame.Find("Item 01")
	if !ok {
		t.Fatal("missing row")
	}
	frame.Move(r.X+r.W/2, r.Y+r.H/2)
	for range 20 {
		frame.Key(0, ui.KeyDown)
	}
	frame.Frame()
	if p.model.Selected != 21 {
		t.Fatalf("scrolling rows under pointer stole keyboard selection: %d", p.model.Selected)
	}
}
