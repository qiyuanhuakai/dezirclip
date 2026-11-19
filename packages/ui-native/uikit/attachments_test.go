package uikit

import (
	"strings"
	"testing"
)

func TestAttachmentNamesAndIconsStayBoundedAndLocal(t *testing.T) {
	rows := ParsePreviewAttachments(`<figure><figcaption>paper &amp; notes.pdf</figcaption></figure><figure><figcaption>table.xlsx</figcaption></figure>`, "")
	if len(rows) != 2 || rows[0].Name != "paper & notes.pdf" || attachmentIconKind(rows[0].Name) != "document" || attachmentIconKind(rows[1].Name) != "spreadsheet" {
		t.Fatal(rows)
	}
	for _, source := range []string{`<img src="https://example.invalid/x">`, `<figure><img src="file:///private"><figcaption>private</figcaption></figure>`} {
		fallback := ParsePreviewAttachments(source, "fallback.zip")
		if len(fallback) != 1 || fallback[0].Name != "fallback.zip" || fallback[0].Bitmap != nil {
			t.Fatal(fallback)
		}
	}
	if len(ParsePreviewAttachments("", strings.Repeat("name.txt\n", 300))) != 256 {
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
