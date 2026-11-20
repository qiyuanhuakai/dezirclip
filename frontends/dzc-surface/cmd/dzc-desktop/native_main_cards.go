package main

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"math"
	"strings"
	"time"
	"unicode/utf16"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type nativeCardKey struct {
	id, timestamp int64
}

func nativeConcealed(row protocol.DesktopEntry) bool {
	if row.Tags == nil {
		return true
	}
	for _, label := range row.Tags {
		if nativeProtectedTag(label) {
			return true
		}
	}
	return false
}

func nativeCardSummary(row protocol.DesktopEntry, language nativeLanguage) string {
	if nativeConcealed(row) {
		return language.text("privacy_sensitive_hidden", "Sensitive content hidden")
	}
	return visibleCardSummary(row, language)
}

func visibleCardSummary(row protocol.DesktopEntry, language nativeLanguage) string {
	if row.ContentType == "image" {
		return language.text("image_preview", "Clipboard image")
	}
	if row.Preview != "" {
		return row.Preview
	}
	return row.Content
}

type nativeCardMedia struct {
	image       ui.ImageSource
	attachments []uikit.PreviewAttachment
}

const (
	historyImageDimension    = 384
	historyImageEncodedLimit = 1024 * 1024
)

func (n *nativeMain) cardBody(c *ui.Context, kit uikit.Kit, row protocol.DesktopEntry, protected *nativeProtectedBody, compact bool, textSize float32, generation uint64) func(float32) {
	if nativeConcealed(row) && protected == nil {
		return nil
	}
	if row.ContentType != "image" && !nativeFileCard(row) {
		return nil
	}
	return func(width float32) {
		if row.ContentType == "image" {
			n.imageCardBody(c, kit, row, protected, width, compact, generation)
			return
		}
		var files []uikit.PreviewAttachment
		if protected != nil {
			files = protected.attachments
		} else if row.HtmlContent != nil && *row.HtmlContent != "" {
			files = uikit.ParsePreviewAttachments(*row.HtmlContent, visibleCardSummary(row, n.language()))
		} else {
			preview, _ := n.mediaForCard(row)
			files = preview.attachments
		}
		if len(files) == 0 {
			files = uikit.ParsePreviewAttachments("", visibleCardSummary(row, n.language()))
		}
		if len(files) == 0 {
			return
		}
		kit.AttachmentGallery(c, files, uikit.AttachmentGalleryOptions{
			Label: n.text("type_file"), Width: width, TextSize: textSize,
			Compact: compact, Video: row.ContentType == "video",
		})
	}
}

func (n *nativeMain) imageCardBody(c *ui.Context, kit uikit.Kit, row protocol.DesktopEntry, protected *nativeProtectedBody, width float32, compact bool, generation uint64) {
	ui.Column(c).FillWidth().Gap(4).Children(func() {
		options := uikit.HistoryImageOptions{Width: width, Compact: compact, Label: n.text("native_image_thumbnail")}
		if row.IsExternal && !row.FilePreviewExists {
			options.Missing = n.text("image_deleted")
		} else {
			var loaded bool
			if protected != nil {
				options.Source, loaded = protected.image, true
			} else {
				preview, ready := n.mediaForCard(row)
				options.Source, loaded = preview.image, ready
			}
			if options.Source == nil && loaded {
				options.Unavailable = n.text("clipboard_image_preview_unavailable")
			}
		}
		kit.HistoryImage(c, options)
		n.drawCardOCR(c, row, protected, generation)
	})
}

func (n *nativeMain) mediaForCard(row protocol.DesktopEntry) (nativeCardMedia, bool) {
	key := nativeCardKey{row.Id, row.Timestamp}
	preview, loaded := n.cardMedia.lookup(key)
	if loaded || n.session == nil || n.library == nil || n.history.state.ServiceInstanceId == "" ||
		row.Id == 0 || !n.cardMedia.begin(key) {
		return preview, loaded
	}
	session, state := n.session, n.history.state
	go func() {
		ctx, cancel := context.WithTimeout(session, 4*time.Second)
		defer cancel()
		size := uint32(480)
		if row.ContentType == "image" {
			size = historyImageDimension
		}
		omitText := true
		result, err := n.library.Entry(ctx, protocol.DesktopEntryParams{
			Id: key.id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch,
			ThumbnailDimension: &size, OmitOcrText: &omitText,
		})
		if err == nil && result.Id == key.id && result.Timestamp == key.timestamp &&
			result.ContentType == row.ContentType && !nativeConcealed(result.DesktopEntry) {
			if row.ContentType == "image" {
				preview.image = decodePreviewSource(result.Content, historyImageEncodedLimit)
			} else if result.HtmlContent != nil {
				preview.attachments = uikit.ParsePreviewAttachments(*result.HtmlContent, result.Preview)
			}
		}
		n.update(session, func() { n.cardMedia.finish(key, preview) })
	}()
	return nativeCardMedia{}, false
}

func (n *nativeMain) cardTags(labels []string) []uikit.HistoryTag {
	if len(labels) == 0 {
		return nil
	}
	if n.cardTagColors == nil && !n.readingTagColors && n.session != nil {
		n.loadCardTagColors()
	}
	results := make([]uikit.HistoryTag, 0, len(labels))
	for _, name := range labels {
		color := nativeTagColor(name, n.themeBase() == "retro")
		if custom, err := hex.DecodeString(strings.TrimPrefix(n.cardTagColors[name], "#")); err == nil && len(custom) == 3 {
			color = ui.RGB(custom[0], custom[1], custom[2])
		}
		results = append(results, uikit.HistoryTag{Name: name, Color: color})
	}
	return results
}

func (n *nativeMain) loadCardTagColors() {
	n.readingTagColors = true
	session, epoch, revision := n.session, n.history.state.PrivacyEpoch, n.tagColorRevision
	go func() {
		ctx, cancel := context.WithTimeout(session, 4*time.Second)
		defer cancel()
		colors := make(map[string]string)
		if raw, err := n.library.TagColors(ctx, epoch); err == nil {
			_ = json.Unmarshal(raw, &colors)
		}
		n.update(session, func() {
			n.readingTagColors = false
			if n.tagColorRevision != revision {
				return
			}
			if colors == nil {
				colors = map[string]string{}
			}
			n.cardTagColors = colors
		})
	}()
}

// Match the palette used by WebView for tags without a saved color.
func nativeTagColor(name string, retro bool) ui.Color {
	var hash int64
	for _, code := range utf16.Encode([]rune(name)) {
		hash = int64(code) + int64(int32(hash)<<5) - hash
	}
	hue := math.Abs(math.Mod(float64(hash)*137.508+float64(int32(hash)>>3), 360)) / 30
	saturation, lightness := .8, .55
	if retro {
		saturation, lightness = .6, .4
	}
	component := func(offset float64) uint8 {
		k := math.Mod(offset+hue, 12)
		value := lightness - saturation*min(lightness, 1-lightness)*max(-1, min(k-3, 9-k, 1))
		return uint8(math.Round(value * 255))
	}
	return ui.RGB(component(0), component(8), component(4))
}
