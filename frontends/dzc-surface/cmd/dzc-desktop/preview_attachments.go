package main

import (
	"bytes"
	"encoding/base64"
	"fmt"
	"html"
	"image"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/egoist/mygo/ui"
)

type previewAttachment struct {
	name  string
	image *ui.Bitmap
}

var attachmentFigure = regexp.MustCompile(`<figure>(?:<img src="(data:image/png;base64,[A-Za-z0-9+/=]+)" alt="[^"]*">)?<figcaption>([^<]*)</figcaption></figure>`)

func previewAttachments(markup, names string) []previewAttachment {
	if len(markup) <= 2*1024*1024 {
		matches := attachmentFigure.FindAllStringSubmatchIndex(markup, 256)
		rows := make([]previewAttachment, 0, len(matches))
		offset := 0
		for _, match := range matches {
			if match[0] != offset {
				break
			}
			row := previewAttachment{name: html.UnescapeString(markup[match[4]:match[5]])}
			if match[2] >= 0 && len(rows) < 4 {
				encoded := strings.TrimPrefix(markup[match[2]:match[3]], "data:image/png;base64,")
				if len(encoded) <= 768*1024 {
					if raw, err := base64.StdEncoding.DecodeString(encoded); err == nil {
						config, _, err := image.DecodeConfig(bytes.NewReader(raw))
						if err == nil && config.Width > 0 && config.Height > 0 && config.Width <= 1024 && config.Height <= 1024 {
							row.image, _ = ui.DecodeBitmap(raw)
						}
						clear(raw)
					}
				}
			}
			rows = append(rows, row)
			offset = match[1]
		}
		if offset == len(markup) && len(rows) > 0 {
			return rows
		}
	}
	var rows []previewAttachment
	for _, name := range strings.Split(names, "\n") {
		if name != "" && len(rows) < 256 {
			rows = append(rows, previewAttachment{name: filepath.Base(strings.ReplaceAll(name, "\\", "/"))})
		}
	}
	return rows
}

func attachmentIconKind(name string) string {
	extension := strings.ToLower(strings.TrimPrefix(filepath.Ext(name), "."))
	for kind, extensions := range map[string]string{
		"image": " png jpg jpeg gif webp bmp avif svg ico tiff ", "video": " mp4 mov mkv webm avi wmv m4v ",
		"audio": " mp3 wav flac ogg m4a aac ", "archive": " zip 7z rar gz tar bz2 xz ",
		"spreadsheet": " xls xlsx csv ods ", "presentation": " ppt pptx odp ",
		"document": " pdf doc docx txt md rtf odt ", "code": " rs go js ts tsx py json html css xml sh ",
	} {
		if extension != "" && strings.Contains(extensions, " "+extension+" ") {
			return kind
		}
	}
	return "file"
}

const (
	attachmentChromeHeight  = 90
	attachmentSummaryHeight = 22
	attachmentRowHeight     = 30
	attachmentMaxNameRows   = 6
)

func attachmentFilenameHeight(count int) int {
	height := attachmentChromeHeight + attachmentSummaryHeight + min(attachmentMaxNameRows, count)*attachmentRowHeight
	if count > 1 {
		height += attachmentSummaryHeight
	}
	return height
}

func attachmentPreviewCapacity(width, height float32, count int, media bool) (tiles, names int, thumbnail float32) {
	// The shared preview chrome occupies at most 90 DIPs. Reserve a final
	// complete text row for overflow instead of clipping the last filename.
	budget := max(float32(0), height-attachmentChromeHeight) - attachmentSummaryHeight
	if count > 1 {
		budget -= attachmentSummaryHeight
	}
	if media && budget >= 110 {
		tiles = min(count, max(1, min(4, int((width-20)/110))))
		thumbnail = min(float32(100), budget-attachmentRowHeight)
		budget -= thumbnail + attachmentRowHeight
	}
	names = min(count-tiles, max(0, min(attachmentMaxNameRows, int(budget/attachmentRowHeight))))
	return
}

func drawAttachmentPreview(c *ui.Context, rows []previewAttachment) {
	media := false
	for _, row := range rows {
		media = media || row.image != nil
	}
	width, height := c.Size()
	shown, names, thumbnail := attachmentPreviewCapacity(width, height, len(rows), media)
	ui.Column(c).Fill().Gap(8).Children(func() {
		if len(rows) > 1 {
			ui.Text(c, fmt.Sprintf("× %d", len(rows))).FontSize(12).TextColor(c.Theme().TextMuted)
		}
		if shown > 0 {
			ui.Row(c).FillWidth().Gap(6).Children(func() {
				for _, row := range rows[:shown] {
					ui.Column(c).Width(0).Grow(1).Gap(4).Children(func() {
						if row.image != nil {
							ui.Image(c, row.image).FillWidth().Height(thumbnail).Fit(ui.Contain)
						}
						drawAttachmentName(c, row.name)
					})
				}
			})
		}
		limit := shown + names
		for _, row := range rows[shown:limit] {
			drawAttachmentName(c, row.name)
		}
		if limit < len(rows) {
			ui.Text(c, "…").FontSize(13).TextColor(c.Theme().TextMuted)
		}
	})
}

func drawAttachmentName(c *ui.Context, name string) {
	ui.Row(c).FillWidth().Height(22).Gap(5).Children(func() {
		previewMetadataIcon(c, attachmentIconKind(name))
		ui.Text(c, name).Grow(1).SingleLine().FontSize(12)
	})
}
