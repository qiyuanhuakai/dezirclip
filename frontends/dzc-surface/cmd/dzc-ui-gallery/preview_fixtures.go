package main

import (
	"fmt"
	"image"
	"image/color"
	"image/draw"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

var picture = samplePicture(160, 96)

func samplePicture(width, height int) *ui.Bitmap {
	bitmap := image.NewRGBA(image.Rect(0, 0, width, height))
	for _, block := range []struct {
		area image.Rectangle
		tint color.RGBA
	}{
		{image.Rect(0, 0, 160, 96), color.RGBA{R: 33, G: 54, B: 90, A: 255}},
		{image.Rect(12, 12, 92, 77), color.RGBA{R: 100, G: 161, B: 177, A: 255}},
		{image.Rect(103, 24, 145, 56), color.RGBA{R: 220, G: 199, B: 138, A: 255}},
		{image.Rect(104, 66, 136, 71), color.RGBA{R: 230, G: 232, B: 240, A: 255}},
	} {
		area := image.Rect(block.area.Min.X*width/160, block.area.Min.Y*height/96,
			block.area.Max.X*width/160, block.area.Max.Y*height/96)
		draw.Draw(bitmap, area, image.NewUniform(block.tint), image.Point{}, draw.Src)
	}
	return ui.NewBitmap(bitmap)
}

func (g *gallery) previewToolbar(c *ui.Context, kit uikit.Kit) {
	kit.PreviewToolbar(c, uikit.PreviewToolbarOptions{
		Title: "Content preview", PinLabel: "Pin preview", CloseLabel: "Close preview", OpacityLabel: "Preview opacity",
		Pinned: g.previewPinned, Opacity: &g.previewOpacity, Image: &g.previewImage,
		ImageLabels: uikit.ImageZoomLabels{In: "Zoom in", Out: "Zoom out", Actual: "Actual size", Fit: "Fit image", FitLabel: "Fit"},
		OnPin:       func() { g.previewPinned = true }, OnClose: func() { g.previewPinned = false },
	})
}

func (g *gallery) imageViewport(c *ui.Context) {
	ui.Column(c).FillWidth().Height(240).Gap(4).Children(func() {
		ui.Text(c, "Image preview").FontSize(16)
		g.previewImage.Draw(c, picture, "Image preview")
	})
}

func (g *gallery) attachmentGallery(c *ui.Context, kit uikit.Kit) {
	files := []uikit.PreviewAttachment{
		{Name: "Project.png"}, {Name: "Screenshot.png"}, {Name: "Notes.pdf"}, {Name: "Planning.xlsx"},
		{Name: "Desktop.zip"}, {Name: "Summary.txt"}, {Name: "Log.txt"},
	}
	compact, expanded := strings.HasPrefix(g.state, "compact"), g.state == "expanded"
	if strings.Contains(g.state, "media") || expanded {
		for i := range files[:2] {
			files[i].Bitmap = picture
		}
	}
	width := float32(480)
	if strings.Contains(g.state, "narrow") {
		width = 220
	}
	ui.Column(c).Width(width).AlignItems(ui.Stretch).Children(func() {
		kit.HistoryCard(c, uikit.HistoryCardOptions{Summary: "Attachment card", Compact: compact, Width: width, Media: func(contentWidth float32) {
			kit.AttachmentGallery(c, files, uikit.AttachmentGalleryOptions{
				Label: "Attachments", Width: contentWidth, Compact: compact, Expanded: expanded,
			})
		}})
	})
}

var historyPictures = []struct {
	label  string
	bitmap *ui.Bitmap
}{{"Small image", samplePicture(32, 24)}, {"Landscape image", picture}, {"Portrait image", samplePicture(72, 230)}}

func inspectMediaCards(fixture string, tester *ui.Tester) any {
	cards := make(map[string]ui.Rect)
	switch fixture {
	case "attachment-preview":
		cards["Attachment card"], _ = tester.Find("Attachment card")
	case "history-rich-text":
		cards["Formatted clipboard card"], _ = tester.Find("Formatted clipboard card")
	default:
		for _, sample := range historyPictures {
			if bounds, ok := tester.Find(sample.label); ok {
				cards[sample.label] = bounds
			}
		}
	}
	return struct {
		Texts []string
		Cards map[string]ui.Rect
	}{tester.Texts(), cards}
}

func (g *gallery) historyImages(c *ui.Context, kit uikit.Kit) {
	compact := strings.HasPrefix(g.state, "compact")
	ui.Column(c).Width(220).Gap(4).AlignItems(ui.Stretch).Children(func() {
		for _, sample := range historyPictures {
			options := uikit.HistoryImageOptions{Label: sample.label, Source: sample.bitmap, Compact: compact}
			switch {
			case strings.HasSuffix(g.state, "missing"):
				options.Source, options.Missing = nil, "Image deleted"
			case strings.HasSuffix(g.state, "unavailable"):
				options.Source, options.Unavailable = nil, "Preview unavailable"
			case strings.HasSuffix(g.state, "pending"):
				options.Source = nil
			}
			kit.HistoryCard(c, uikit.HistoryCardOptions{
				Source: "Editor", Time: "12:30", Summary: sample.label, Width: 220, Compact: compact,
				Actions: []uikit.HeaderAction{{Icon: "pin", Label: "Pin"}, {Icon: "tag", Label: "Tags"}, {Icon: "hide", Label: "Delete"}},
				Media: func(width float32) {
					options.Width = width
					kit.HistoryImage(c, options)
				},
			})
			if options.Source == nil {
				break
			}
		}
	})
}

func (g *gallery) inputRegion(c *ui.Context, wheel bool) {
	panel := ui.Column(c).Size(320, 144).Padding(12).Border(1, c.Theme().Accent).Radius(8).Label("Input target")
	panel.Children(func() {
		if wheel {
			uikit.WheelHotZone(c, func(ui.InputEvent) { g.wheelCount++ }, func() {
				ui.Text(c, "Wheel input").FontSize(16)
				message := "Scroll over this area"
				if g.wheelCount != 0 {
					message = fmt.Sprintf("%d scroll", g.wheelCount)
				}
				ui.Text(c, message).FontSize(16)
			})
			return
		}
		ui.Text(c, "Resize preview").FontSize(16)
		ui.Text(c, string(g.resizeDirection)).FontSize(16)
		uikit.PreviewResizeHandles(c, "Resize preview", func(direction uikit.PreviewResizeDirection) {
			g.resizeDirection = direction
		})
	})
}
