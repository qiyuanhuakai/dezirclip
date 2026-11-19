package main

import (
	"bytes"
	"image"
	"image/color"
	"image/draw"
	"image/png"
	"os"
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

func TestAttachmentPreviewVisualFixture(t *testing.T) {
	directory := os.Getenv("DZC_VISUAL_DIR")
	if directory == "" {
		t.Skip("visual export directory is not set")
	}
	if err := os.MkdirAll(directory, 0700); err != nil {
		t.Fatal(err)
	}
	rows := uikit.ParsePreviewAttachments("", "paper.pdf\nmeasurements.xlsx\npresentation.pptx\narchive.zip\nsong.flac\nmain.rs\nphoto.png\nclip.mp4\nextra.txt")
	media := append([]uikit.PreviewAttachment(nil), rows...)
	for i, shade := range []color.RGBA{{20, 120, 220, 255}, {20, 180, 100, 255}, {220, 90, 40, 255}, {140, 70, 200, 255}} {
		pixels := image.NewRGBA(image.Rect(0, 0, 120, 80))
		draw.Draw(pixels, pixels.Bounds(), image.NewUniform(shade), image.Point{}, draw.Src)
		draw.Draw(pixels, image.Rect(0, 70, 120, 80), image.NewUniform(color.RGBA{245, 245, 245, 255}), image.Point{}, draw.Src)
		var encoded bytes.Buffer
		if err := png.Encode(&encoded, pixels); err != nil {
			t.Fatal(err)
		}
		media[i].Bitmap, _ = ui.DecodeBitmap(encoded.Bytes())
	}
	for _, content := range []struct {
		label string
		rows  []uikit.PreviewAttachment
	}{{"files", rows}, {"media", media}} {
		for _, dark := range []bool{false, true} {
			mode := "light"
			if dark {
				mode = "dark"
			}
			preview := &pinnedPreview{ticket: pinPreviewRequest{Appearance: previewAppearance{Dark: dark, Opacity: 95, Title: "Content preview", Close: "Close preview"}},
				opacity: previewOpacityControl{value: 95, slider: 95}, body: &previewBody{attachments: content.rows, kind: "files", source: "DezirClip File Drop", timestamp: 1794222000000}}
			frame := ui.NewTester(preview.view, 480, 320).Image()
			file, err := os.Create(filepath.Join(directory, "attachments-"+content.label+"-"+mode+".png"))
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
}
