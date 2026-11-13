package main

import (
	"bytes"
	"image"
	"image/color"
	"image/draw"
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestAttachmentNamesAndIconsStayBoundedAndLocal(t *testing.T) {
	rows := previewAttachments(`<figure><figcaption>paper &amp; notes.pdf</figcaption></figure><figure><figcaption>table.xlsx</figcaption></figure>`, "")
	if len(rows) != 2 || rows[0].name != "paper & notes.pdf" || attachmentIconKind(rows[0].name) != "document" || attachmentIconKind(rows[1].name) != "spreadsheet" {
		t.Fatal(rows)
	}
	for _, source := range []string{`<img src="https://example.invalid/x">`, `<figure><img src="file:///private"><figcaption>private</figcaption></figure>`} {
		fallback := previewAttachments(source, "fallback.zip")
		if len(fallback) != 1 || fallback[0].name != "fallback.zip" || fallback[0].image != nil {
			t.Fatal(fallback)
		}
	}
	if len(previewAttachments("", strings.Repeat("name.txt\n", 300))) != 256 {
		t.Fatal("unbounded attachment list")
	}
}

func TestAttachmentCapacityReservesCompleteOverflowRows(t *testing.T) {
	for _, height := range []float32{176, 240, 320, 600} {
		for _, media := range []bool{false, true} {
			tiles, names, thumbnail := attachmentPreviewCapacity(480, height, 30, media)
			used := float32(44 + names*30)
			if tiles > 0 {
				used += thumbnail + 30
			}
			if tiles > 4 || names > 6 || used > height-90 {
				t.Fatalf("height=%v media=%v: %d tiles, %d names use %v", height, media, tiles, names, used)
			}
		}
	}
}

func TestAttachmentPreviewVisualFixture(t *testing.T) {
	directory := os.Getenv("DZC_VISUAL_DIR")
	if directory == "" {
		t.Skip("visual export directory is not set")
	}
	if err := os.MkdirAll(directory, 0700); err != nil {
		t.Fatal(err)
	}
	rows := previewAttachments("", "paper.pdf\nmeasurements.xlsx\npresentation.pptx\narchive.zip\nsong.flac\nmain.rs\nphoto.png\nclip.mp4\nextra.txt")
	media := append([]previewAttachment(nil), rows...)
	for i, shade := range []color.RGBA{{20, 120, 220, 255}, {20, 180, 100, 255}, {220, 90, 40, 255}, {140, 70, 200, 255}} {
		pixels := image.NewRGBA(image.Rect(0, 0, 120, 80))
		draw.Draw(pixels, pixels.Bounds(), image.NewUniform(shade), image.Point{}, draw.Src)
		draw.Draw(pixels, image.Rect(0, 70, 120, 80), image.NewUniform(color.RGBA{245, 245, 245, 255}), image.Point{}, draw.Src)
		var encoded bytes.Buffer
		if err := png.Encode(&encoded, pixels); err != nil {
			t.Fatal(err)
		}
		media[i].image, _ = ui.DecodeBitmap(encoded.Bytes())
	}
	for _, content := range []struct {
		label string
		rows  []previewAttachment
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
