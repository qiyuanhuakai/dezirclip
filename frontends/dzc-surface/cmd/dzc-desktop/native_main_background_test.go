package main

import (
	"bytes"
	"encoding/base64"
	"image"
	"image/color"
	"image/draw"
	"image/gif"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestNativeBackgroundLayerFollowsThemeAndAuthorizedEpoch(t *testing.T) {
	imageData := image.NewRGBA(image.Rect(0, 0, 40, 30))
	shade := color.RGBA{R: 220, G: 10, B: 130, A: 255}
	draw.Draw(imageData, imageData.Bounds(), image.NewUniform(shade), image.Point{}, draw.Src)
	bitmap := ui.NewBitmap(imageData)
	for _, theme := range []string{"mica", "acrylic", "liquid-glass", "macos", "retro", "scifi"} {
		t.Run(theme, func(t *testing.T) {
			n := nativeMain{theme: theme, mode: "light", preferences: map[string]string{
				"app.language":    "en",
				backgroundSetting: "fixture.png", "app.custom_background_opacity": "100", "app.surface_opacity": "0",
			}}
			n.history.state = protocol.RuntimeState{ServiceInstanceId: "host", PrivacyEpoch: 3}
			n.background = nativeBackground{key: nativeBackgroundKey{"fixture.png", "host", 3}, bitmap: bitmap}
			tester := ui.NewTester(n.view, 360, 420)
			sample := func() color.RGBA { return tester.Image().RGBAAt(320, 385) }
			supported := theme == "mica" || theme == "acrylic" || theme == "liquid-glass"
			if (sample() == shade) != supported {
				t.Fatalf("theme %s wallpaper sample is %v", theme, sample())
			}
			if !supported {
				return
			}
			if err := tester.Click("Settings"); err != nil || !n.showSettings {
				t.Fatalf("the foreground settings button is inaccessible over the wallpaper: %v", err)
			}
			n.closeSettings()
			n.preferences["app.custom_background_opacity"] = "50"
			tester.Frame()
			if alpha := sample().A; alpha < 127 || alpha > 128 {
				t.Fatalf("wallpaper alpha = %d, want 50%%", alpha)
			}
			n.history.state.PrivacyEpoch++
			tester.Frame()
			if alpha := sample().A; alpha != 0 {
				t.Fatalf("old wallpaper survived the privacy epoch: %v", sample())
			}
		})
	}
}

func TestNativeBackgroundGIFCompositionAndLoops(t *testing.T) {
	red, green, blue := color.RGBA{R: 255, A: 255}, color.RGBA{G: 255, A: 255}, color.RGBA{B: 255, A: 255}
	palette := color.Palette{red, green, blue}
	a := image.NewPaletted(image.Rect(0, 0, 3, 2), palette)
	b := image.NewPaletted(image.Rect(1, 0, 2, 2), palette)
	c := image.NewPaletted(image.Rect(2, 0, 3, 2), palette)
	b.Pix[0], b.Pix[1], c.Pix[0], c.Pix[1] = 1, 1, 2, 2
	var encoded bytes.Buffer
	if err := gif.EncodeAll(&encoded, &gif.GIF{
		Image: []*image.Paletted{a, b, c}, Delay: []int{0, 4, 5}, LoopCount: 1,
		Disposal: []byte{gif.DisposalNone, gif.DisposalPrevious, gif.DisposalBackground},
	}); err != nil {
		t.Fatal(err)
	}
	payload := base64.StdEncoding.EncodeToString(encoded.Bytes())
	if bitmap, _ := decodeNativeBackground("data:image/png;base64," + payload); bitmap != nil {
		t.Fatal("incompatible MIME type was accepted")
	}
	bitmap, animation := decodeNativeBackground("data:image/gif;base64," + payload)
	if bitmap == nil || animation == nil {
		t.Fatal("animated wallpaper was dropped")
	}
	if delay, ok := animation.nextDelay(); delay != 100*time.Millisecond || !ok {
		t.Fatalf("zero-delay frame was not normalized: %s, %v", delay, ok)
	}
	if sample := animation.advance().RGBAAt(1, 0); sample != green {
		t.Fatalf("offset frame is %v, want green", sample)
	}
	frame := animation.advance()
	if frame.RGBAAt(1, 0) != red || frame.RGBAAt(2, 0) != blue {
		t.Fatal("restore-previous disposal did not restore the first frame behind the current frame")
	}
	frame = animation.advance()
	if frame.RGBAAt(1, 0) != red || frame.RGBAAt(2, 0) != red {
		t.Fatal("loop did not begin with the original first frame")
	}
	animation.advance()
	animation.advance()
	if _, ok := animation.nextDelay(); ok {
		t.Fatal("finite animation started an extra loop")
	}
	if backgroundGIFWithinBudget(encoded.Bytes()[:encoded.Len()-2]) {
		t.Fatal("truncated animation was accepted")
	}
}
