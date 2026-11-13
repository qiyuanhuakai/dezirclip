package main

import (
	"bytes"
	"encoding/base64"
	"image"
	"math"
	"strings"

	"github.com/egoist/mygo/ui"
)

func previewSourceImage(source string) ui.ImageSource {
	header, encoded, ok := strings.Cut(source, ",")
	if !ok || !strings.HasPrefix(header, "data:image/") || !strings.HasSuffix(header, ";base64") || len(encoded) > 384*1024 {
		return nil
	}
	raw, err := base64.StdEncoding.DecodeString(encoded)
	if err != nil {
		return nil
	}
	defer clear(raw)
	if header == "data:image/svg+xml;base64" {
		vector, err := ui.ParseSVG(raw)
		if err != nil {
			return nil
		}
		width, height := vector.Size()
		if width <= 0 || height <= 0 || width > 4096 || height > 4096 || math.IsNaN(float64(width)) || math.IsNaN(float64(height)) {
			return nil
		}
		return vector
	}
	dimensions, _, err := image.DecodeConfig(bytes.NewReader(raw))
	if err != nil || dimensions.Width <= 0 || dimensions.Height <= 0 || dimensions.Width > 512 || dimensions.Height > 512 {
		return nil
	}
	bitmap, err := ui.DecodeBitmap(raw)
	if err != nil {
		return nil
	}
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
		case "spreadsheet":
			line(2, 2, 12, 2, 12, 12, 2, 12, 2, 2)
			line(2, 5, 12, 5)
			line(2, 8.5, 12, 8.5)
			line(6, 2, 6, 12)
		case "archive":
			line(3, 1.5, 11, 1.5, 11, 12.5, 3, 12.5, 3, 1.5)
			line(6, 2, 8, 2, 8, 4, 6, 4, 6, 6, 8, 6, 8, 8)
			line(6, 9, 8, 9, 8, 11, 6, 11, 6, 9)
		case "audio":
			line(5, 10, 5, 3, 11, 1.5, 11, 8.5)
			line(5, 10, 4, 12, 2, 12, 1.5, 10.5, 3, 9.5, 5, 10)
			line(11, 8.5, 10, 10.5, 8, 10.5, 7.5, 9, 9, 8, 11, 8.5)
		case "code":
			line(4, 4, 1, 7, 4, 10)
			line(10, 4, 13, 7, 10, 10)
			line(8, 2, 6, 12)
		case "presentation":
			line(1, 2, 13, 2, 13, 10, 1, 10, 1, 2)
			line(7, 10, 7, 13)
			line(4, 13, 7, 11, 10, 13)
			line(4, 8, 6, 5, 8, 7, 10, 4)
		default:
			line(3, 1.5, 8, 1.5, 11, 4.5, 11, 12.5, 3, 12.5, 3, 1.5)
			line(8, 1.5, 8, 4.5, 11, 4.5)
			line(5, 7, 9, 7)
			line(5, 9.5, 9, 9.5)
		}
		p.StrokePath(&path, 1.15, color)
	})
}
