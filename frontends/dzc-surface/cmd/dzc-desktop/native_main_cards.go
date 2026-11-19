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

func (n *nativeMain) cardBody(c *ui.Context, row protocol.DesktopEntry, protected *nativeProtectedBody, compact bool, generation uint64) func() {
	if nativeConcealed(row) {
		if protected != nil {
			if row.ContentType == "image" {
				return func() { n.imageCardBody(c, row, protected, compact, generation) }
			}
			if len(protected.attachments) != 0 {
				return func() { uikit.HistoryAttachments(c, protected.attachments, compact) }
			}
		}
		return nil
	}
	switch row.ContentType {
	case "image":
		return func() { n.imageCardBody(c, row, nil, compact, generation) }
	case "file", "files", "video":
		attachments := uikit.ParsePreviewAttachments("", visibleCardSummary(row, n.language()))
		if len(attachments) != 0 {
			return func() { uikit.HistoryAttachments(c, attachments, compact) }
		}
	}
	return nil
}

func (n *nativeMain) imageCardBody(c *ui.Context, row protocol.DesktopEntry, protected *nativeProtectedBody, compact bool, generation uint64) {
	ui.Column(c).FillWidth().Gap(4).Children(func() {
		if protected != nil {
			n.drawHistoryImage(c, protected.image, true, compact)
		} else {
			n.historyImage(c, row, compact)
		}
		n.drawCardOCR(c, row, protected, generation)
	})
}

func (n *nativeMain) historyImage(c *ui.Context, row protocol.DesktopEntry, compact bool) {
	key := nativeCardKey{row.Id, row.Timestamp}
	preview, loaded := n.cardImages.lookup(key)
	if !loaded {
		n.loadCardImage(key)
	}
	n.drawHistoryImage(c, preview, loaded, compact)
}

func (n *nativeMain) drawHistoryImage(c *ui.Context, preview ui.ImageSource, loaded, compact bool) {
	width, height := float32(178), float32(100)
	if compact {
		width, height = 78, 44
	}
	if image, ok := preview.(*ui.Bitmap); ok {
		w, h := image.Size()
		width = min(width, max(height*.45, height*float32(w)/float32(max(h, 1))))
	}
	box := ui.Box(c).Width(width).MaxWidthPercent(100).Height(height).
		Border(1, c.Theme().Border).Radius(min(4, c.Theme().Radius)).Center()
	box.Children(func() {
		switch {
		case preview != nil:
			ui.Image(c, preview).Fill().Fit(ui.Contain).Label(n.text("native_image_thumbnail"))
		case loaded:
			ui.Text(c, n.text("clipboard_image_preview_unavailable")).FontSize(11).TextColor(c.Theme().TextMuted).Wrap()
		}
	})
}

func (n *nativeMain) loadCardImage(key nativeCardKey) {
	if n.session == nil || !n.cardImages.begin(key) {
		return
	}
	session, state := n.session, n.history.state
	go func() {
		ctx, cancel := context.WithTimeout(session, 4*time.Second)
		defer cancel()
		size := uint32(192)
		omitText := true
		row, err := n.library.Entry(ctx, protocol.DesktopEntryParams{
			Id: key.id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, ThumbnailDimension: &size, OmitOcrText: &omitText,
		})
		var preview ui.ImageSource
		if err == nil && row.Timestamp == key.timestamp && row.ContentType == "image" && !nativeConcealed(row.DesktopEntry) {
			preview = previewSourceImage(row.Content)
		}
		n.update(session, func() { n.cardImages.finish(key, preview) })
	}()
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
