package main

import (
	"context"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/deziro-ui/packages/ui-native/uikit"
)

type nativeProtectedRow struct {
	ctx    context.Context
	cancel context.CancelFunc
	body   *nativeProtectedBody
}

type nativeProtectedBody struct {
	summary     string
	image       ui.ImageSource
	attachments []uikit.PreviewAttachment
	rich        *uikit.HistoryRichText
	ocrStatus   string
}

func (n *nativeMain) protectedBody(row protocol.DesktopEntry) *nativeProtectedBody {
	request := n.protectedRows[nativeCardKey{row.Id, row.Timestamp}]
	if request == nil {
		return nil
	}
	return request.body
}

func (n *nativeMain) canUseProtectedRow(row protocol.DesktopEntry) bool {
	return n.protectedBody(row) != nil && n.window != nil && mainWindowFocused(n.window)
}

func (n *nativeMain) forgetProtectedRow(key nativeCardKey) {
	if request := n.protectedRows[key]; request != nil {
		request.cancel()
		delete(n.protectedRows, key)
		if n.ocrPreview != nil && n.ocrPreview.key == key {
			n.closeOCRPreview()
		}
		if n.preview != nil && n.preview.entryID == key.id {
			n.closePreview()
		}
	}
}

func (n *nativeMain) clearProtectedRows() bool {
	if n.tagLibrary != nil {
		n.tagLibrary.reveal = nil
	}
	hadRows := len(n.protectedRows) != 0
	for key := range n.protectedRows {
		n.forgetProtectedRow(key)
	}
	return hadRows
}

func (n *nativeMain) pruneProtectedRows(first, last int) {
	first, last = max(0, first), min(last, len(n.history.rows)-1)
	for key := range n.protectedRows {
		visible := false
		for index := first; index <= last; index++ {
			row := n.history.rows[index]
			if row.Id == key.id && row.Timestamp == key.timestamp {
				visible = true
				break
			}
		}
		if !visible {
			n.forgetProtectedRow(key)
		}
	}
}

func (n *nativeMain) protectedRowAction(row protocol.DesktopEntry, generation uint64) uikit.HeaderAction {
	key := nativeCardKey{row.Id, row.Timestamp}
	icon, label := "reveal", n.text("native_reveal_sensitive")
	if n.protectedRows[key] != nil {
		icon, label = "conceal", n.text("native_hide_sensitive")
	}
	return uikit.HeaderAction{
		Icon: icon, Label: label, Active: icon == "conceal", Disabled: n.mutating || n.failed,
		Run: func() {
			if tags := n.tagLibrary; tags != nil && tags.reveal != nil {
				tags.reveal[key] = n.protectedRows[key] != nil
			}
			n.toggleProtectedRow(row, generation, key)
		},
	}
}

func (n *nativeMain) revealTaggedRow(row protocol.DesktopEntry, generation uint64) {
	tags := n.tagLibrary
	if tags == nil || tags.reveal == nil || tags.revealGeneration != generation || n.mutating || n.failed {
		return
	}
	key := nativeCardKey{row.Id, row.Timestamp}
	if tags.reveal[key] || n.protectedRows[key] != nil {
		return
	}
	n.toggleProtectedRow(row, generation, key)
}

func (n *nativeMain) toggleProtectedRow(row protocol.DesktopEntry, generation uint64, key nativeCardKey) {
	if n.session == nil || generation != n.history.generation || n.failed || n.mutating || !nativeConcealed(row) {
		return
	}
	if n.protectedRows[key] != nil {
		n.forgetProtectedRow(key)
		return
	}
	state := n.history.state
	if n.window == nil || !mainWindowFocused(n.window) || state.ServiceInstanceId == "" {
		return
	}
	session, language := n.session, n.language()
	lifetime, cancel := context.WithCancel(session)
	request := &nativeProtectedRow{ctx: lifetime, cancel: cancel}
	if n.protectedRows == nil {
		n.protectedRows = make(map[nativeCardKey]*nativeProtectedRow)
	}
	n.protectedRows[key] = request
	n.notice = ""
	go func() {
		ctx, cancel := context.WithTimeout(lifetime, 4*time.Second)
		defer cancel()
		params := protocol.DesktopEntryParams{
			Id: key.id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, Reveal: true,
		}
		if row.ContentType == "image" {
			size := uint32(historyImageDimension)
			omitText := true
			params.ThumbnailDimension = &size
			params.OmitOcrText = &omitText
		}
		if row.ContentType == "rich_text" {
			size := uint32(historyRichImageDimension)
			params.ThumbnailDimension = &size
		}
		result, err := n.library.Entry(ctx, params)
		var body *nativeProtectedBody
		if err == nil && ctx.Err() == nil && result.Timestamp == key.timestamp {
			body = newProtectedBody(result.DesktopEntry, language)
		}
		n.update(session, func() {
			if n.protectedRows[key] != request {
				return
			}
			if !mainWindowFocused(n.window) || n.history.generation != generation {
				n.forgetProtectedRow(key)
				return
			}
			if body == nil {
				n.forgetProtectedRow(key)
				if tags := n.tagLibrary; tags != nil && tags.reveal != nil && tags.revealGeneration == generation {
					tags.reveal[key] = true
				}
				n.notice = n.text("privacy_action_failed", "Could not reveal the sensitive entry")
				return
			}
			request.body = body
		})
	}()
}

func newProtectedBody(row protocol.DesktopEntry, language nativeLanguage) *nativeProtectedBody {
	summary := visibleCardSummary(row, language)
	for index := range summary {
		if index >= 4096 {
			summary = strings.Clone(summary[:index]) + "…"
			break
		}
	}
	body := &nativeProtectedBody{summary: strings.Clone(summary)}
	switch row.ContentType {
	case "image":
		body.image = decodePreviewSource(row.Content, historyImageEncodedLimit)
		if row.OcrStatus != nil {
			body.ocrStatus = *row.OcrStatus
		}
	case "file", "files", "video":
		markup := ""
		if row.HtmlContent != nil {
			markup = *row.HtmlContent
		}
		body.attachments = uikit.ParsePreviewAttachments(markup, row.Preview)
	case "rich_text":
		if row.HtmlContent != nil {
			body.rich = uikit.ParseHistoryRichText(*row.HtmlContent)
		}
	}
	return body
}
