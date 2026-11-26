package main

import (
	"math"
	"strconv"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/uikit"
)

func (body *previewBody) mediaBitmap() *ui.Bitmap {
	if body.bitmap != nil {
		return body.bitmap
	}
	if len(body.attachments) == 1 {
		return body.attachments[0].Bitmap
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

func (body *previewBody) filenameWidth(family string) int {
	width := float32(0)
	font := ui.Font{Family: family, Size: 12}
	for _, item := range body.attachments {
		if item.Bitmap != nil {
			return 0
		}
		w, _ := ui.ShapeText(item.Name, font, 0).Size()
		width = max(width, w)
	}
	if width == 0 {
		return 0
	}
	return max(320, int(math.Ceil(float64(width)))+48)
}

func (body *previewBody) hoverSize(area mygo.Rectangle, theme *ui.Theme) (int, int) {
	maxWidth, maxHeight := min(560, area.Width), min(560, area.Height)
	if fitted := body.filenameWidth(theme.Font); fitted > 0 {
		return min(maxWidth, fitted), min(maxHeight, uikit.AttachmentFilenameHeight(len(body.attachments)))
	}
	if len(body.attachments) > 1 {
		width := min(480, maxWidth)
		return width, min(maxHeight, uikit.AttachmentGalleryHeight(width, len(body.attachments)))
	}
	if body.formatted != nil && body.mediaBitmap() == nil && len(body.attachments) == 0 {
		return body.textSize(theme, maxWidth, maxHeight)
	}
	return body.preferredSize(area)
}

type previewParagraphSize struct {
	spans          []ui.Span
	font           ui.Font
	inset, spacing float32
	width, height  float32
	codeBlock      bool
}

// Measure the same paragraph fonts and insets as the editor. Fit the frame
// before the flyout opens, and cap the work for very long documents.
func (body *previewBody) textSize(theme *ui.Theme, maxWidth, maxHeight int) (int, int) {
	look, base := richtext.DefaultTheme(theme), theme.FontSize
	naturalWidth, budget := float32(0), 8192
	var paragraphs []previewParagraphSize
	for _, paragraph := range body.formatted.Text().Paragraphs() {
		if len(paragraphs) >= 256 {
			return maxWidth, maxHeight
		}
		style := paragraph.Style
		size := previewParagraphSize{
			font:      ui.Font{Family: theme.Font, Size: base},
			inset:     float32(style.Indent) * base * look.Indent,
			codeBlock: style.CodeBlock,
			spans:     make([]ui.Span, 0, len(paragraph.Runs)),
		}
		if style.Heading > 0 {
			heading := look.Headings[min(style.Heading, 6)-1]
			size.font.Size *= heading.Size
			size.font.Weight = heading.Weight
			size.spacing = base * heading.Bottom
			if len(paragraphs) > 0 {
				size.spacing += size.font.Size * heading.Top
			}
		} else if style.CodeBlock {
			size.font.Family = look.CodeFont
			size.font.Size *= look.CodeSize
		}
		if style.List != richtext.NoList {
			size.inset += base * look.Indent
		}
		if style.Quote {
			size.inset += base * look.Indent * .75
		}
		previousCode := len(paragraphs) > 0 && paragraphs[len(paragraphs)-1].codeBlock
		codePadding := base * look.CodeBlockPadding[1]
		if style.CodeBlock {
			size.inset += base * look.CodeBlockPadding[0] * 2
			if !previousCode {
				size.spacing += codePadding
			}
		} else if previousCode {
			paragraphs[len(paragraphs)-1].spacing += codePadding
		}
		for _, run := range paragraph.Runs {
			budget -= len(run.Text)
			if budget < 0 {
				return maxWidth, maxHeight
			}
			a := run.Attributes
			span := ui.Span{Text: run.Text, Font: a.Font, Size: a.Size, Weight: a.Weight, Italic: a.Italic}
			if a.Code {
				span.Font = look.CodeFont
				if span.Size == 0 {
					span.Size = base * look.CodeSize
				}
			}
			if a.Attachment != nil {
				span.ObjectWidth, span.ObjectHeight = previewInlineAttachmentSize(a.Attachment)
			}
			size.spans = append(size.spans, span)
		}
		size.width, size.height = ui.ShapeRichText(size.spans, size.font, 0).Size()
		naturalWidth = max(naturalWidth, size.width+size.inset)
		paragraphs = append(paragraphs, size)
	}
	if last := len(paragraphs) - 1; last >= 0 && paragraphs[last].codeBlock {
		paragraphs[last].spacing += base * look.CodeBlockPadding[1]
	}

	// Leave horizontal room for panel padding, scrollbar and glyph overhang.
	width := min(maxWidth, max(320, int(math.Ceil(float64(naturalWidth)))+40))
	height := float32(90) // Panel padding, toolbar, metadata and their spacing.
	for _, paragraph := range paragraphs {
		wrapWidth := max(1, float32(width-32)-paragraph.inset)
		lineHeight := paragraph.height
		if paragraph.width > wrapWidth {
			for i := range paragraph.spans {
				span := &paragraph.spans[i]
				if span.ObjectWidth > wrapWidth {
					span.ObjectHeight *= wrapWidth / span.ObjectWidth
					span.ObjectWidth = wrapWidth
				}
			}
			_, lineHeight = ui.ShapeRichText(paragraph.spans, paragraph.font, wrapWidth).Size()
		}
		height += max(lineHeight, paragraph.font.Size) + paragraph.spacing
		if height >= float32(maxHeight) {
			return width, maxHeight
		}
	}
	return width, min(maxHeight, max(112, int(math.Ceil(float64(height)))))
}

func previewInlineAttachmentSize(attachment *richtext.Attachment) (float32, float32) {
	width, height := attachment.Width, attachment.Height
	var imageWidth, imageHeight float32
	switch source := attachment.Image.(type) {
	case *ui.Bitmap:
		w, h := source.Size()
		imageWidth, imageHeight = float32(w), float32(h)
	case *ui.SVG:
		imageWidth, imageHeight = source.Size()
	}
	switch {
	case width <= 0 && height <= 0:
		width, height = imageWidth, imageHeight
	case width <= 0 && imageHeight > 0:
		width = height * imageWidth / imageHeight
	case height <= 0 && imageWidth > 0:
		height = width * imageHeight / imageWidth
	}
	return max(width, 1), max(height, 1)
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
