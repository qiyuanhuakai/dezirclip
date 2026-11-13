package main

import (
	"bytes"
	"encoding/base64"
	"image"
	"math"
	"strings"

	"github.com/egoist/mygo/ui"
)

func previewSourceBitmap(source string) *ui.Bitmap {
	header, encoded, ok := strings.Cut(source, ",")
	if !ok || !strings.HasPrefix(header, "data:image/") || !strings.HasSuffix(header, ";base64") || len(encoded) > 384*1024 {
		return nil
	}
	raw, err := base64.StdEncoding.DecodeString(encoded)
	if err != nil {
		return nil
	}
	defer clear(raw)
	dimensions, _, err := image.DecodeConfig(bytes.NewReader(raw))
	if err != nil || dimensions.Width <= 0 || dimensions.Height <= 0 || dimensions.Width > 512 || dimensions.Height > 512 {
		return nil
	}
	bitmap, _ := ui.DecodeBitmap(raw)
	return bitmap
}

func previewMetadataIcon(c *ui.Context, kind string) {
	color := c.Theme().TextMuted
	ui.Box(c).Size(14, 14).Draw(func(p *ui.Painter, r ui.Rect) {
		var path ui.Path
		line := func(points ...float32) {
			path.MoveTo(r.X+points[0], r.Y+points[1])
			for i := 2; i < len(points); i += 2 {
				path.LineTo(r.X+points[i], r.Y+points[i+1])
			}
		}
		switch kind {
		case "clock":
			for i := 0; i <= 24; i++ {
				angle := float64(i) * math.Pi / 12
				x, y := r.X+7+float32(math.Cos(angle))*5.4, r.Y+7+float32(math.Sin(angle))*5.4
				if i == 0 {
					path.MoveTo(x, y)
				} else {
					path.LineTo(x, y)
				}
			}
			line(7, 3.5, 7, 7, 9.5, 8.5)
		case "image":
			line(2, 2, 12, 2, 12, 12, 2, 12, 2, 2)
			line(2, 10, 5, 7, 7, 9, 9, 6, 12, 9)
			line(4, 4, 5, 4)
		case "video":
			line(2, 2, 12, 2, 12, 12, 2, 12, 2, 2)
			line(5, 4, 10, 7, 5, 10, 5, 4)
		case "application":
			line(2, 2, 12, 2, 12, 12, 2, 12, 2, 2)
			line(2, 5, 12, 5)
			line(4, 3.5, 4.3, 3.5)
		default:
			line(3, 1.5, 8, 1.5, 11, 4.5, 11, 12.5, 3, 12.5, 3, 1.5)
			line(8, 1.5, 8, 4.5, 11, 4.5)
			line(5, 7, 9, 7)
			line(5, 9.5, 9, 9.5)
		}
		p.StrokePath(&path, 1.15, color)
	})
}
