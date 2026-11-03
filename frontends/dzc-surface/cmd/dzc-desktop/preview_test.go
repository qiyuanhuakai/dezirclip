package main

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"
)

func TestLongRichPreviewOpensAtFirstParagraph(t *testing.T) {
	const opening = "First paragraph"
	doc := richPreviewDocument("<p>" + opening + "</p>" + strings.Repeat("<p>Later paragraph</p>", 40))
	view := ui.NewTester(func(c *ui.Context) { richtext.Editor(c, doc, richtext.EditorOptions{ReadOnly: true}).Fill() }, 360, 160)
	view.ClickAt(30, 12)
	anchor, caret := doc.Selection()
	if anchor != caret || caret <= 0 || caret > len(opening) {
		t.Fatalf("hit near the top of the first frame selected position %d:%d, outside the first paragraph", anchor, caret)
	}
}
