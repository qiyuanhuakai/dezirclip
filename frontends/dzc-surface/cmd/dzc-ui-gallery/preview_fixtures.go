package main

import (
	"fmt"
	"image"
	"image/color"
	"image/draw"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

var picture = func() *ui.Bitmap {
	bitmap := image.NewRGBA(image.Rect(0, 0, 160, 96))
	for _, block := range []struct {
		area image.Rectangle
		tint color.RGBA
	}{
		{bitmap.Bounds(), color.RGBA{R: 33, G: 54, B: 90, A: 255}},
		{image.Rect(12, 12, 92, 77), color.RGBA{R: 100, G: 161, B: 177, A: 255}},
		{image.Rect(103, 24, 145, 56), color.RGBA{R: 220, G: 199, B: 138, A: 255}},
		{image.Rect(104, 66, 136, 71), color.RGBA{R: 230, G: 232, B: 240, A: 255}},
	} {
		draw.Draw(bitmap, block.area, image.NewUniform(block.tint), image.Point{}, draw.Src)
	}
	return ui.NewBitmap(bitmap)
}()

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
	compact, expanded := g.state == "compact", g.state == "expanded"
	if g.state == "media" || expanded {
		for i := range files[:2] {
			files[i].Bitmap = picture
		}
	}
	width, _ := c.Size()
	kit.HistoryCard(c, uikit.HistoryCardOptions{Compact: compact, Body: func() {
		kit.AttachmentGallery(c, files, uikit.AttachmentGalleryOptions{
			Label: "Attachments", Width: width - 80, Compact: compact, Expanded: expanded,
		})
	}})
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
