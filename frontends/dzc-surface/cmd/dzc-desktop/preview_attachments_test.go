package main

import (
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

func TestAttachmentPreviewVisualFixture(t *testing.T) {
	directory := os.Getenv("DZC_VISUAL_DIR")
	if directory == "" {
		t.Skip("visual export directory is not set")
	}
	if err := os.MkdirAll(directory, 0700); err != nil {
		t.Fatal(err)
	}
	rows := previewAttachments("", "paper.pdf\nmeasurements.xlsx\npresentation.pptx\narchive.zip\nsong.flac\nmain.rs\nphoto.png\nclip.mp4\nextra.txt")
	for _, dark := range []bool{false, true} {
		mode := "light"
		if dark {
			mode = "dark"
		}
		appearance := previewAppearance{Dark: dark, Opacity: 95}
		frame := ui.NewTester(func(c *ui.Context) { appearance.panel(c).Children(func() { drawAttachmentPreview(c, rows) }) }, 360, 260).Image()
		file, err := os.Create(filepath.Join(directory, "attachments-"+mode+".png"))
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
