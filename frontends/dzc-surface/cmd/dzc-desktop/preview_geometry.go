package main

import (
	"math"
	"strconv"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
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
