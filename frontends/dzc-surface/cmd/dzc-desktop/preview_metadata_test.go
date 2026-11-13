package main

import (
	"bytes"
	"encoding/base64"
	"github.com/egoist/mygo/ui"
	"image"
	"image/png"
	"testing"
)

func TestPreviewSourceImageSupportsRasterAndVector(t *testing.T) {
	var pngBytes bytes.Buffer
	if err := png.Encode(&pngBytes, image.NewRGBA(image.Rect(0, 0, 16, 16))); err != nil {
		t.Fatal(err)
	}
	if _, ok := previewSourceImage("data:image/png;base64," + base64.StdEncoding.EncodeToString(pngBytes.Bytes())).(*ui.Bitmap); !ok {
		t.Fatal("PNG source icon missing")
	}
	svg := []byte(`<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><path fill="#04f" d="M0 0H32V32H0Z"/></svg>`)
	if _, ok := previewSourceImage("data:image/svg+xml;base64," + base64.StdEncoding.EncodeToString(svg)).(*ui.SVG); !ok {
		t.Fatal("Linux SVG source icon missing")
	}
	for _, source := range []string{"https://example.invalid/icon.png", "data:image/png;base64,bm90LXBpY3R1cmU=", "data:image/svg+xml;base64,eA=="} {
		if previewSourceImage(source) != nil {
			t.Fatal("invalid source icon must use the fallback")
		}
	}
}
