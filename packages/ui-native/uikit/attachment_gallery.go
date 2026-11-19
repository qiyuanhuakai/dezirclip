package uikit

import (
	"fmt"
	"math"
	"slices"

	"github.com/egoist/mygo/ui"
)

type AttachmentGalleryOptions struct {
	Label    string
	Width    float32
	Compact  bool
	Expanded bool
	Video    bool
}

var attachmentSymbols = map[string]*ui.SVG{
	"image": icon("Image"), "video": icon("Video"), "audio": icon("Music"),
	"archive": icon("FileArchive"), "code": icon("FileCode"), "spreadsheet": icon("FileSpreadsheet"),
	"presentation": icon("FileChartColumn"), "document": icon("FileText"), "file": icon("File"),
}

type attachmentGalleryLayout struct {
	count, columns, overflowNames, lastSpan, omitted int
	media                                            bool
}

func layoutAttachmentGallery(files []PreviewAttachment, width float32, compact, expanded bool) attachmentGalleryLayout {
	media := slices.ContainsFunc(files, func(file PreviewAttachment) bool { return file.Bitmap != nil })
	rows, cellWidth := 4, float32(134)
	if expanded {
		rows = 8
	} else if compact {
		rows = 2
	}
	if media {
		cellWidth = 116
		if compact && !expanded {
			cellWidth = 78
		}
	}
	available := min(4, max(1, int((max(1, width)+6)/cellWidth)))
	columns := 1
	if media {
		columns = min(available, max(1, len(files)))
	} else if !expanded {
		columns = min(available, max(1, (len(files)+rows-1)/rows))
	}
	capacity := columns * rows
	if media {
		capacity = columns
	}
	layout := attachmentGalleryLayout{media: media, columns: columns, count: min(capacity, len(files)), lastSpan: 1}
	if !media {
		if len(files) > capacity {
			layout.count--
		} else if layout.count != 0 {
			layout.lastSpan = columns - (layout.count-1)%columns
		}
	} else {
		nameCapacity := columns
		if expanded {
			nameCapacity = 6
		}
		remaining := len(files) - layout.count
		layout.overflowNames = remaining
		if remaining > nameCapacity {
			layout.overflowNames = nameCapacity - 1
		}
	}
	layout.omitted = len(files) - layout.count - layout.overflowNames
	return layout
}

// AttachmentGallery displays compact history thumbnails or the expanded file list.
// Width is the space available to the gallery in DIPs, after parent padding.
func (k Kit) AttachmentGallery(c *ui.Context, files []PreviewAttachment, options AttachmentGalleryOptions) {
	if !options.Expanded {
		firstVideo := slices.ContainsFunc(files, func(file PreviewAttachment) bool {
			return file.Bitmap != nil && (options.Video || attachmentIconKind(file.Name) == "video")
		})
		if firstVideo {
			files = slices.Clone(files)
			for i := range files {
				if options.Video || attachmentIconKind(files[i].Name) == "video" {
					files[i].Bitmap = nil
				}
			}
		}
	}
	layout := layoutAttachmentGallery(files, options.Width, options.Compact, options.Expanded)
	labelStyle := k.style("attachmentName")
	if options.Expanded {
		labelStyle.Size = 12
	}
	iconStyle := k.style("attachmentIcon")
	mediaStyle := k.style("attachmentImage")
	cellWidth := (max(1, options.Width) - float32(layout.columns-1)*6) / float32(layout.columns)
	ui.Column(c).FillWidth().Gap(4).Label(fmt.Sprintf("%s × %d", options.Label, len(files))).Children(func() {
		drawGrid := func(items []PreviewAttachment, thumbnails, showOverflow bool, lastSpan int) {
			count := len(items)
			if showOverflow {
				count++
			}
			grid := ui.Grid(c).FillWidth().Columns(layout.columns)
			grid.Children(func() {
				for index := 0; index < count; index++ {
					cell := ui.Column(c).MinWidth(0).Gap(4)
					if index == len(items)-1 {
						cell.ColumnSpan(lastSpan)
					}
					if index%layout.columns != 0 {
						cell.Margin(0, 0, 0, 6)
					}
					cell.Children(func() {
						if index == len(items) {
							text(c, "…", labelStyle).TextColor(styleColor(iconStyle.Text))
							return
						}
						file := items[index]
						if thumbnails && file.Bitmap != nil {
							width, height := file.Bitmap.Size()
							limit := float32(100)
							if options.Compact {
								limit = 44
							}
							if options.Expanded {
								ui.Image(c, file.Bitmap).FillWidth().Height(160).Fit(ui.Contain).Radius(4)
							} else if width > 0 && height > 0 {
								scale := min(1, cellWidth/float32(width), limit/float32(height))
								paint(ui.Image(c, file.Bitmap).Size(float32(math.Round(float64(float32(width)*scale))),
									float32(math.Round(float64(float32(height)*scale)))).Fit(ui.Contain), mediaStyle)
							}
							if !options.Expanded {
								return
							}
						}
						ui.Row(c).FillWidth().Height(max(16, labelStyle.Height)).AlignItems(ui.Center).Gap(5).Tooltip(file.Name).Children(func() {
							symbol := attachmentSymbols[attachmentIconKind(file.Name)]
							ui.Icon(c, symbol).Size(16, 16).TextColor(styleColor(iconStyle.Text))
							text(c, file.Name, labelStyle).Grow(1).SingleLine()
						})
					})
				}
			})
		}
		drawGrid(files[:layout.count], layout.media, !layout.media && layout.omitted > 0, layout.lastSpan)
		if layout.media && (layout.overflowNames > 0 || layout.omitted > 0) {
			drawGrid(files[layout.count:layout.count+layout.overflowNames], false, layout.omitted > 0, 1)
		}
	})
}
