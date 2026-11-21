package uikit

import (
	"bytes"
	"encoding/base64"
	"image"
	"image/png"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestHistoryRichTextRetainsLocalFormattingAndOneBoundedImage(t *testing.T) {
	var picture bytes.Buffer
	if err := png.Encode(&picture, image.NewRGBA(image.Rect(0, 0, 12, 8))); err != nil {
		t.Fatal(err)
	}
	embedded := `<img src="data:image/png;base64,` + base64.StdEncoding.EncodeToString(picture.Bytes()) + `" alt="Example image">`
	preview := ParseHistoryRichText(`<p><strong>Bold &amp; bright</strong> <em>italic</em></p>
<script>Script content</script><img src="https://example.invalid/ignored" alt="Remote image">` + embedded + embedded)
	if preview == nil {
		t.Fatal("the rich preview was empty")
	}
	shown := preview.text.String()
	if !strings.Contains(shown, "Bold & bright") || strings.Contains(shown, "Script content") || strings.Contains(shown, "Remote image") {
		t.Fatalf("unexpected preview text %q", shown)
	}
	bold, italic, images := false, false, 0
	for _, paragraph := range preview.text.Paragraphs() {
		for _, run := range paragraph.Runs {
			bold = bold || strings.Contains(run.Text, "Bold") && run.Attributes.Bold()
			italic = italic || strings.Contains(run.Text, "italic") && run.Attributes.Italic
			if run.Attributes.Attachment != nil && run.Attributes.Attachment.Image != nil {
				images++
			}
		}
	}
	if !bold || !italic || images != 1 {
		t.Fatalf("preview attributes: bold %t, italic %t, embedded images %d", bold, italic, images)
	}
	if ParseHistoryRichText(`<img src="https://example.invalid/picture" alt="Remote image">`) != nil {
		t.Fatal("remote-only HTML must fall back to the summary")
	}
}

func TestHistoryRichTextCardClipsBodyAndKeepsCardActivation(t *testing.T) {
	preview := ParseHistoryRichText("<p><strong>Release checklist</strong></p>" + strings.Repeat("<p>A second paragraph</p>", 20))
	for _, compact := range []bool{false, true} {
		activated := 0
		view := ui.NewTester(func(c *ui.Context) {
			Kit{}.HistoryCard(c, HistoryCardOptions{Summary: "Rich clipboard entry", Rich: preview, Compact: compact,
				Activate: func() { activated++ }})
		}, 340, 200)
		card, ok := view.Find("Rich clipboard entry")
		limit := float32(100)
		if compact {
			limit = 75
		}
		if !ok || card.H > limit {
			t.Fatalf("rich card expanded past its height cap in compact=%t: %+v", compact, card)
		}
		if err := view.Click("Release checklist"); err != nil || activated != 1 {
			t.Fatalf("rich contents intercepted the card action: %d, %v", activated, err)
		}
	}
}
