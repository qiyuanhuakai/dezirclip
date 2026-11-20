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
	return decodePreviewSource(source, 384*1024)
}

func decodePreviewSource(source string, maxEncoded int) ui.ImageSource {
	header, encoded, ok := strings.Cut(source, ",")
	if !ok || !strings.HasPrefix(header, "data:image/") || !strings.HasSuffix(header, ";base64") || len(encoded) > maxEncoded {
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
