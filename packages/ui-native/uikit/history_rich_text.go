package uikit

import (
	"encoding/base64"
	"image"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"
	"github.com/egoist/mygo/ui/shader"
	"golang.org/x/net/html"
	"golang.org/x/net/html/atom"
)

// HistoryRichText is a bounded, read-only rendition of a clipboard HTML fragment.
type HistoryRichText struct {
	text richtext.Text
}

var historyRichFade = shader.MustCompile(`package main
func ColorEffect(pos vec2, color vec4) vec4 {
	height := rectSize().y
	return color * clamp((height - pos.y) / (height * 0.30), 0, 1)
}
`).Effect(shader.Options{})

// ParseHistoryRichText accepts local formatting and a small embedded preview image.
// A nil result lets the card show its ordinary text summary.
func ParseHistoryRichText(markup string) *HistoryRichText {
	if len(markup) > 2<<20 || strings.TrimSpace(markup) == "" {
		return nil
	}
	tokenizer := html.NewTokenizer(strings.NewReader(markup))
	var fragment strings.Builder
	hasImage := false
	for kind := tokenizer.Next(); kind != html.ErrorToken; kind = tokenizer.Next() {
		if kind == html.StartTagToken || kind == html.SelfClosingTagToken {
			token := tokenizer.Token()
			if token.DataAtom == atom.Img {
				if !hasImage && smallHistoryRichImage(token.Attr) {
					fragment.WriteString(token.String())
					hasImage = true
				}
			} else {
				fragment.WriteString(token.String())
			}
			continue
		}
		fragment.Write(tokenizer.Raw())
	}
	contents := richtext.ParseHTML(fragment.String())
	if strings.TrimSpace(contents.String()) == "" {
		return nil
	}
	return &HistoryRichText{text: contents.Slice(0, min(1600, contents.Len()))}
}

func smallHistoryRichImage(attributes []html.Attribute) bool {
	for _, attribute := range attributes {
		if attribute.Key != "src" {
			continue
		}
		header, content, ok := strings.Cut(attribute.Val, ",")
		if !ok || !strings.HasPrefix(header, "data:image/") || !strings.HasSuffix(header, ";base64") || len(content) > 256<<10 {
			return false
		}
		config, _, err := image.DecodeConfig(base64.NewDecoder(base64.StdEncoding, strings.NewReader(content)))
		return err == nil && config.Width > 0 && config.Height > 0 && config.Width <= 480 && config.Height <= 480
	}
	return false
}

func (k Kit) historyRichText(c *ui.Context, options HistoryCardOptions, width float32) {
	widget := "historyRichContent"
	if options.Compact {
		widget += "Compact"
	}
	style := k.style(widget)
	if options.Compact {
		if outer := k.style("historyContentCompact").MaxHeight; outer > 0 {
			style.MaxHeight = min(style.MaxHeight, max(1, outer-style.MarginTop-style.MarginBottom))
		}
	}
	if options.TextSize > 0 {
		style.Size = options.TextSize
	}
	mono := k.monospaceFont
	if mono == "" {
		mono = "monospace"
	}
	paint(ui.Column(c).FillWidth().MinWidth(0).MaxHeight(style.MaxHeight).Clip().PassThrough(), style).
		Margin(style.MarginTop, 0, style.MarginBottom).Effect(historyRichFade).Children(func() {
		number := 0
		for index, paragraph := range options.Rich.text.Paragraphs() {
			if index == 8 {
				break
			}
			if paragraph.Style.List == richtext.Numbers {
				number++
			} else {
				number = 0
			}
			spans := richParagraphPrefix(paragraph.Style, number)
			draw := func() {
				if len(spans) == 0 {
					return
				}
				row := styledText(ui.RichText(c, spans...), style).FillWidth().MinWidth(0).
					Wrap().TextAlign(paragraph.Style.Align).PassThrough()
				if paragraph.Style.Heading > 0 {
					row.FontWeight(700).Margin(2, 0)
				}
				spans = nil
			}
			for _, run := range paragraph.Runs {
				a := run.Attributes
				if a.Attachment != nil && a.Attachment.Image != nil {
					draw()
					imageStyle := style
					imageStyle.MaxHeight = 80
					w, h := historyImageSize(a.Attachment.Image, width, imageStyle)
					preview := ui.Image(c, a.Attachment.Image).Size(w, h).Fit(ui.Contain).AlignSelf(ui.Start).PassThrough()
					if a.Attachment.Name != "" {
						preview.Label(a.Attachment.Name)
					}
					continue
				}
				if a.Attachment != nil {
					continue
				}
				span := ui.Span{Text: run.Text, Weight: a.Weight, Italic: a.Italic,
					Underline: a.Underline || a.Link != "", Strikethrough: a.Strikethrough}
				if a.Link != "" {
					span.Color = c.Theme().Accent
				}
				if a.Code || paragraph.Style.CodeBlock {
					span.Font = mono
				}
				spans = append(spans, span)
			}
			draw()
		}
	})
}

func richParagraphPrefix(style richtext.ParagraphStyle, number int) []ui.Span {
	prefix := ""
	switch style.List {
	case richtext.Bullets:
		prefix = "• "
	case richtext.Numbers:
		prefix = strconv.Itoa(number) + ". "
	case richtext.Tasks:
		prefix = "☐ "
		if style.Checked {
			prefix = "☑ "
		}
	}
	if prefix == "" {
		return nil
	}
	return []ui.Span{{Text: strings.Repeat("  ", style.Indent) + prefix}}
}
