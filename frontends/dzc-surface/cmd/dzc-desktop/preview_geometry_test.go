package main

import (
	"context"
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
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
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
		body := &previewBody{attachments: []uikit.PreviewAttachment{{Name: "portrait.mp4", Bitmap: bitmap}}}
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
		short = (&previewBody{attachments: []uikit.PreviewAttachment{{Name: "a.txt"}}}).filenameWidth(c.Theme().Font)
		long = (&previewBody{attachments: []uikit.PreviewAttachment{{Name: "A very long document filename that requires more space than a short file.txt"}}}).filenameWidth(c.Theme().Font)
	}, 560, 320)
	if short != 320 || long <= short {
		t.Fatalf("widths do not follow filenames: %d %d", short, long)
	}
}

func TestTextPreviewFitsPlainAndStyledParagraphs(t *testing.T) {
	var short, medium, long, multiline, normal, heading, large, indented int
	ui.NewTester(func(c *ui.Context) {
		plain := func(text string) int {
			width, _ := (&previewBody{formatted: previewDocument(richtext.NewText(text))}).textSize(c.Theme(), 560, 560)
			return width
		}
		html := func(text string) int {
			width, _ := (&previewBody{formatted: previewDocument(richtext.ParseHTML(text))}).textSize(c.Theme(), 560, 560)
			return width
		}
		short = plain("短文字")
		medium = plain(strings.Repeat("M", 32))
		long = plain(strings.Repeat("M", 9000))
		multiline = plain("第一行\n第二行\n第三行")
		normal = html("<p>" + strings.Repeat("M", 24) + "</p>")
		heading = html("<h1>" + strings.Repeat("M", 24) + "</h1>")
		large, _ = (&previewBody{formatted: previewDocument(richtext.TextOf(richtext.Paragraph{Runs: []richtext.Run{{Text: strings.Repeat("M", 24), Attributes: richtext.Attributes{Size: 30}}}}))}).textSize(c.Theme(), 560, 560)
		indented = html("<blockquote>" + strings.Repeat("M", 32) + "</blockquote>")
	}, 560, 320)
	if short != 320 || multiline != short || medium <= short || medium >= 560 || long != 560 {
		t.Fatalf("plain widths: %d %d %d %d", short, medium, long, multiline)
	}
	if heading <= normal || large <= normal || indented <= medium {
		t.Fatalf("styles lost: normal=%d heading=%d large=%d indented=%d", normal, heading, large, indented)
	}
}

func TestHoverPreviewHeightFollowsTextLayout(t *testing.T) {
	area := mygo.Rectangle{Width: 1920, Height: 1080}
	style := (&previewAppearance{Opacity: 65}).theme()
	plain := func(text string) *previewBody {
		return &previewBody{formatted: previewDocument(richtext.NewText(text))}
	}
	_, short := plain("One line").hoverSize(area, style)
	_, several := plain(strings.Repeat("Another line\n", 8)).hoverSize(area, style)
	_, many := plain(strings.Repeat("Another line\n", 80)).hoverSize(area, style)
	_, styled := (&previewBody{formatted: previewDocument(richtext.ParseHTML("<h1>Heading</h1><p>Body</p>"))}).hoverSize(area, style)
	if short >= several || several >= many || many != 560 || styled <= short {
		t.Fatalf("unexpected fitted heights: short=%d several=%d long=%d formatted=%d", short, several, many, styled)
	}
	wrapped := plain(strings.Repeat("word with several letters ", 12))
	wideWidth, wide := wrapped.hoverSize(area, style)
	narrowWidth, narrow := wrapped.hoverSize(mygo.Rectangle{Width: 320, Height: 1080}, style)
	if narrowWidth >= wideWidth || narrow <= wide {
		t.Fatalf("a narrower preview did not grow after wrapping: %dx%d vs %dx%d", narrowWidth, narrow, wideWidth, wide)
	}
	_, limited := wrapped.hoverSize(mygo.Rectangle{Width: 320, Height: 135}, style)
	if limited != 135 {
		t.Fatalf("preview ignored the monitor height: %d", limited)
	}
}

func TestPreviewFitsNextToSourceAtEdges(t *testing.T) {
	row := mygo.Rectangle{X: 450, Y: 235, Width: 330, Height: 36}
	area := mygo.Rectangle{Width: 800, Height: 580}
	preferred := mygo.Point{X: 792, Y: 235}
	left := previewSizedBounds(area, row, preferred, 350, 120)
	if left.Width != 350 || left.X+left.Width != row.X-4 {
		t.Fatalf("preview did not switch to the adjacent left side: %+v", left)
	}
	row.X, row.Width = 5, 780
	below := previewSizedBounds(area, row, preferred, 350, 120)
	if below.X < row.X || below.X+below.Width > row.X+row.Width || below.Y != row.Y+row.Height+4 {
		t.Fatalf("vertical preview did not line up with the source: %+v", below)
	}
}

func TestHoverPreviewFitsAttachmentGalleryRows(t *testing.T) {
	area := mygo.Rectangle{Width: 1920, Height: 1080}
	appearance := previewAppearance{Title: "Content preview", Opacity: 65}
	bitmap := ui.NewBitmap(image.NewRGBA(image.Rect(0, 0, 30, 30)))
	for _, count := range []int{2, 9} {
		rows := make([]uikit.PreviewAttachment, count)
		for i := range rows {
			rows[i] = uikit.PreviewAttachment{Name: fmt.Sprintf("File %d.png", i), Bitmap: bitmap}
		}
		body := &previewBody{attachments: rows}
		width, height := body.hoverSize(area, appearance.theme())
		panel := &pinnedPreview{body: body, ticket: pinPreviewRequest{Appearance: appearance}}
		view := ui.NewTester(panel.view, width, height)
		for _, row := range rows {
			if bounds, ok := view.Find(row.Name); !ok || bounds.Y+bounds.H > float32(height) {
				t.Fatalf("%s did not fit in %dx%d: %+v", row.Name, width, height, bounds)
			}
		}
	}
}

func TestNativeHoverRetainsPreviewWhileCrossingAnotherCard(t *testing.T) {
	session, cancel := context.WithCancel(context.Background())
	defer cancel()
	n := nativeMain{
		session: session, window: new(mygo.Window),
		preview: &Preview{entryID: 7, generation: 3, window: new(mygo.Window)},
		hover:   nativeHoverPreview{row: 7, preview: 3},
	}
	defer n.hover.reset()
	bounds := ui.Rect{X: 8, Y: 24, W: 100, H: 32}
	n.hoverPreview(protocol.DesktopEntry{Id: 8}, 0, bounds)
	timer := n.hover.timer
	if timer == nil || !n.hover.owns(n.preview) {
		t.Fatal("leaving the source card immediately closed its preview")
	}
	n.hoverPreview(protocol.DesktopEntry{Id: 9}, 0, bounds)
	if n.hover.timer != timer {
		t.Fatal("passing another card restarted the handoff delay")
	}
	n.hoverPreview(protocol.DesktopEntry{Id: 7}, 0, bounds)
	if n.hover.timer != nil || !n.hover.owns(n.preview) {
		t.Fatal("returning to the source did not cancel the handoff")
	}
}

func TestFittedFilenameGalleryActuallyDrawsNames(t *testing.T) {
	for _, count := range []int{1, 2, 6, 30} {
		rows := make([]uikit.PreviewAttachment, count)
		for i := range rows {
			rows[i].Name = fmt.Sprintf("document-%d.txt", i)
		}
		body := &previewBody{kind: "files", source: "DezirClip File Drop", attachments: rows}
		height := uikit.AttachmentFilenameHeight(count)
		p := &pinnedPreview{body: body, opacity: previewOpacityControl{value: 85, slider: 85},
			ticket: pinPreviewRequest{Appearance: previewAppearance{Title: "Content preview", Opacity: 85}}}
		view := ui.NewTester(p.view, 320, height)
		for _, row := range rows[:min(count, uikit.AttachmentMaxNameRows)] {
			rect, found := view.Find(row.Name)
			if !found || rect.W <= 0 || rect.H <= 0 || rect.Y < 0 || rect.Y+rect.H > float32(height) {
				t.Fatalf("%d files: %s missing or clipped at height %d: %+v", count, row.Name, height, rect)
			}
		}
		if view.HasText("…") != (count > uikit.AttachmentMaxNameRows) {
			t.Fatalf("%d files: incorrect overflow", count)
		}
	}
}

func TestPinnedVideoFirstFrameUsesFullPortraitViewport(t *testing.T) {
	picture := image.NewRGBA(image.Rect(0, 0, 320, 960))
	draw.Draw(picture, picture.Bounds(), image.NewUniform(color.RGBA{40, 110, 180, 255}), image.Point{}, draw.Src)
	body := &previewBody{kind: "video", source: "DezirClip", attachments: []uikit.PreviewAttachment{{Name: "portrait.mp4", Bitmap: ui.NewBitmap(picture)}}}
	width, height := body.preferredSize(mygo.Rectangle{Width: 1920, Height: 1080})
	p := &pinnedPreview{body: body, opacity: previewOpacityControl{value: 85, slider: 85},
		ticket: pinPreviewRequest{Appearance: previewAppearance{Title: "Content preview", Language: "en", Opacity: 85, FitLabel: "Fit"}}}
	frame := ui.NewTester(p.view, width, height).Image()
	_, viewportHeight := p.imageView.ViewportSize()
	if viewportHeight < 300 || p.imageView.FitZoom() <= 0 || frame.RGBAAt(width/2, height/2).B < 100 {
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
