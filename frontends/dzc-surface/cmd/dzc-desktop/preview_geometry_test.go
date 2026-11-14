package main

import (
	"fmt"
	"image"
	"image/color"
	"image/draw"
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"
)

func TestMediaPreviewFollowsPortraitLandscapeAndSmallImages(t *testing.T) {
	for _, size := range [][2]int{{1080, 1920}, {1920, 1080}, {100, 100}, {4000, 3000}} {
		width, height := previewMediaSize(size[0], size[1], 560, 560)
		if width > 560 || height > 560 || width < 320 {
			t.Fatalf("unbounded frame %dx%d", width, height)
		}
		if size[0] < size[1] && height <= width {
			t.Fatalf("portrait remains landscape: %dx%d", width, height)
		}
		if size[0] == 100 && height != 190 {
			t.Fatalf("small image stretched: %dx%d", width, height)
		}
		bitmap := ui.NewBitmap(image.NewRGBA(image.Rect(0, 0, size[0]/10, size[1]/10)))
		body := &previewBody{attachments: []previewAttachment{{name: "portrait.mp4", image: bitmap}}}
		if body.mediaBitmap() == nil {
			t.Fatal("single video frame must use image viewport")
		}
		if w, h := body.preferredSize(mygo.Rectangle{Width: 200, Height: 150}); w > 200 || h > 150 {
			t.Fatal("small display overflow")
		}
	}
}

func TestGalleryWidthMeasuresFilenames(t *testing.T) {
	var short, long int
	ui.NewTester(func(c *ui.Context) {
		short = (&previewBody{attachments: []previewAttachment{{name: "a.txt"}}}).filenameWidth(c)
		long = (&previewBody{attachments: []previewAttachment{{name: "A very long document filename that requires more space than a short file.txt"}}}).filenameWidth(c)
	}, 560, 320)
	if short != 320 || long <= short {
		t.Fatalf("widths do not follow filenames: %d %d", short, long)
	}
}

func TestTextPreviewFitsPlainAndStyledParagraphs(t *testing.T) {
	var short, medium, long, multiline, normal, heading, large, indented int
	ui.NewTester(func(c *ui.Context) {
		plain := func(text string) int {
			return (&previewBody{formatted: previewDocument(richtext.NewText(text))}).textWidth(c)
		}
		html := func(text string) int {
			return (&previewBody{formatted: previewDocument(richtext.ParseHTML(text))}).textWidth(c)
		}
		short = plain("短文字")
		medium = plain(strings.Repeat("M", 32))
		long = plain(strings.Repeat("M", 9000))
		multiline = plain("第一行\n第二行\n第三行")
		normal = html("<p>" + strings.Repeat("M", 24) + "</p>")
		heading = html("<h1>" + strings.Repeat("M", 24) + "</h1>")
		large = (&previewBody{formatted: previewDocument(richtext.TextOf(richtext.Paragraph{Runs: []richtext.Run{{Text: strings.Repeat("M", 24), Attributes: richtext.Attributes{Size: 30}}}}))}).textWidth(c)
		indented = html("<blockquote>" + strings.Repeat("M", 32) + "</blockquote>")
	}, 560, 320)
	if short != 320 || multiline != short || medium <= short || medium >= 560 || long != 560 {
		t.Fatalf("plain widths: %d %d %d %d", short, medium, long, multiline)
	}
	if heading <= normal || large <= normal || indented <= medium {
		t.Fatalf("styles lost: normal=%d heading=%d large=%d indented=%d", normal, heading, large, indented)
	}
}

func TestFittedFilenameGalleryActuallyDrawsNames(t *testing.T) {
	for _, count := range []int{1, 2, 6, 30} {
		rows := make([]previewAttachment, count)
		for i := range rows {
			rows[i].name = fmt.Sprintf("document-%d.txt", i)
		}
		body := &previewBody{kind: "files", source: "DezirClip File Drop", attachments: rows}
		height := attachmentFilenameHeight(count)
		p := &pinnedPreview{body: body, opacity: previewOpacityControl{value: 85, slider: 85},
			ticket: pinPreviewRequest{Appearance: previewAppearance{Title: "Content preview", Opacity: 85}}}
		view := ui.NewTester(p.view, 320, height)
		for _, row := range rows[:min(count, attachmentMaxNameRows)] {
			rect, found := view.Find(row.name)
			if !found || rect.W <= 0 || rect.H <= 0 || rect.Y < 0 || rect.Y+rect.H > float32(height) {
				t.Fatalf("%d files: %s missing or clipped at height %d: %+v", count, row.name, height, rect)
			}
		}
		if view.HasText("…") != (count > attachmentMaxNameRows) {
			t.Fatalf("%d files: incorrect overflow", count)
		}
	}
}

func TestPinnedVideoFirstFrameUsesFullPortraitViewport(t *testing.T) {
	picture := image.NewRGBA(image.Rect(0, 0, 320, 960))
	draw.Draw(picture, picture.Bounds(), image.NewUniform(color.RGBA{40, 110, 180, 255}), image.Point{}, draw.Src)
	body := &previewBody{kind: "video", source: "DezirClip", attachments: []previewAttachment{{name: "portrait.mp4", image: ui.NewBitmap(picture)}}}
	width, height := body.preferredSize(mygo.Rectangle{Width: 1920, Height: 1080})
	p := &pinnedPreview{body: body, opacity: previewOpacityControl{value: 85, slider: 85},
		ticket: pinPreviewRequest{Appearance: previewAppearance{Title: "Content preview", Language: "en", Opacity: 85, FitLabel: "Fit"}}}
	frame := ui.NewTester(p.view, width, height).Image()
	if p.imageView.height < 300 || p.imageView.fit <= 0 || frame.RGBAAt(width/2, height/2).B < 100 {
		t.Fatalf("video frame did not fill portrait viewport: %+v", p.imageView)
	}
	if directory := os.Getenv("DZC_VISUAL_DIR"); directory != "" {
		if err := os.MkdirAll(directory, 0700); err != nil {
			t.Fatal(err)
		}
		file, err := os.Create(filepath.Join(directory, "native-video-portrait.png"))
		if err != nil {
			t.Fatal(err)
		}
		defer file.Close()
		if err := png.Encode(file, frame); err != nil {
			t.Fatal(err)
		}
	}
}

func TestNativePreviewTimeMatchesWebviewThresholds(t *testing.T) {
	now := time.Unix(1700000000, 0)
	for _, c := range []struct {
		seconds int64
		zh, en  string
	}{
		{0, "< 1分钟", "< 1m"}, {59, "< 1分钟", "< 1m"}, {60, "1分钟前", "1m ago"},
		{3599, "59分钟前", "59m ago"}, {3600, "1小时前", "1h ago"}, {86400, "1天前", "1d ago"},
	} {
		timestamp := now.UnixMilli() - c.seconds*1000
		if got := concisePreviewTime(timestamp, now, "zh"); got != c.zh {
			t.Fatalf("zh: %s", got)
		}
		for _, language := range []string{"en", "tw"} {
			if got := concisePreviewTime(timestamp, now, language); got != c.en {
				t.Fatalf("%s: %s", language, got)
			}
		}
	}
}
