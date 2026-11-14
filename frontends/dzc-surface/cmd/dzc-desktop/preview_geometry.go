package main

import (
	"math"
	"strconv"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"
)

func (body *previewBody) mediaBitmap() *ui.Bitmap {
	if body.bitmap != nil {
		return body.bitmap
	}
	if len(body.attachments) == 1 {
		return body.attachments[0].image
	}
	return nil
}

// Reserve room for controls and metadata; never stretch a small picture.
func previewMediaSize(width, height, maxWidth, maxHeight int) (int, int) {
	if width < 1 || height < 1 {
		return min(480, maxWidth), min(320, maxHeight)
	}
	scale := min(1.0, float64(max(1, maxWidth-24))/float64(width), float64(max(1, maxHeight-90))/float64(height))
	return min(maxWidth, max(320, int(math.Ceil(float64(width)*scale))+24)),
		min(maxHeight, int(math.Ceil(float64(height)*scale))+90)
}

func (body *previewBody) preferredSize(area mygo.Rectangle) (int, int) {
	width, height := min(560, area.Width), min(560, area.Height)
	if bitmap := body.mediaBitmap(); bitmap != nil {
		w, h := bitmap.Size()
		return previewMediaSize(int(w), int(h), width, height)
	}
	return min(480, width), min(320, height)
}

func (body *previewBody) filenameWidth(c *ui.Context) int {
	width := float32(0)
	for _, item := range body.attachments {
		if item.image != nil {
			return 0
		}
		w, _ := c.MeasureText(0, ui.Span{Text: item.name, Size: 12})
		width = max(width, w)
	}
	if width == 0 {
		return 0
	}
	return max(320, int(math.Ceil(float64(width)))+48)
}

// Match the editor's paragraph and run fonts, without laying out the whole
// document again on every frame. Long documents use the bounded wide frame.
func (body *previewBody) textWidth(c *ui.Context) int {
	if body.formatted == nil || body.bitmap != nil || len(body.attachments) > 0 {
		return 0
	}
	look := richtext.DefaultTheme(c.Theme())
	base := c.Theme().FontSize
	width, budget := float32(0), 8192
	for index, paragraph := range body.formatted.Text().Paragraphs() {
		if index >= 256 {
			return 560
		}
		style := paragraph.Style
		size, weight, font := base, 400, ""
		if style.Heading > 0 {
			heading := look.Headings[min(style.Heading, 6)-1]
			size, weight = base*heading.Size, heading.Weight
		} else if style.CodeBlock {
			size, font = base*look.CodeSize, look.CodeFont
		}
		inset := float32(style.Indent) * base * look.Indent
		if style.List != richtext.NoList {
			inset += base * look.Indent
		}
		if style.Quote {
			inset += base * look.Indent * 0.75
		}
		if style.CodeBlock {
			inset += base * look.CodeBlockPadding[0] * 2
		}
		spans := make([]ui.Span, 0, len(paragraph.Runs))
		for _, run := range paragraph.Runs {
			budget -= len(run.Text)
			if budget < 0 || run.Attributes.Attachment != nil {
				return 560
			}
			a := run.Attributes
			span := ui.Span{Text: run.Text, Font: font, Size: size, Weight: weight, Italic: a.Italic}
			if a.Font != "" {
				span.Font = a.Font
			}
			if a.Size > 0 {
				span.Size = a.Size
			}
			if a.Weight > 0 {
				span.Weight = a.Weight
			}
			if a.Code {
				span.Font = look.CodeFont
				if a.Size == 0 {
					span.Size = base * look.CodeSize
				}
			}
			spans = append(spans, span)
		}
		measured, _ := c.MeasureText(0, spans...)
		width = max(width, measured+inset)
		if width >= 520 {
			return 560
		}
	}
	// Panel padding, editor scrollbar and a little room for italic overhang.
	return min(560, max(320, int(math.Ceil(float64(width)))+40))
}

func concisePreviewTime(timestamp int64, now time.Time, language string) string {
	seconds := (now.UnixMilli() - timestamp) / 1000
	if language == "zh" {
		if seconds < 60 {
			return "< 1分钟"
		}
		if seconds < 3600 {
			return strconv.FormatInt(seconds/60, 10) + "分钟前"
		}
		if seconds < 86400 {
			return strconv.FormatInt(seconds/3600, 10) + "小时前"
		}
		return strconv.FormatInt(seconds/86400, 10) + "天前"
	}
	if seconds < 60 {
		return "< 1m"
	}
	if seconds < 3600 {
		return strconv.FormatInt(seconds/60, 10) + "m ago"
	}
	if seconds < 86400 {
		return strconv.FormatInt(seconds/3600, 10) + "h ago"
	}
	return strconv.FormatInt(seconds/86400, 10) + "d ago"
}
