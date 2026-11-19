package main

import (
	"context"
	"errors"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type nativeOCRPreview struct {
	key     nativeCardKey
	cancel  context.CancelFunc
	text    string
	loading bool
	failed  bool
}

type nativeOCRJob struct {
	id     int64
	status string
}

func (n *nativeMain) ocrStatus(row protocol.DesktopEntry, body *nativeProtectedBody) string {
	if n.ocrJob.id == row.Id {
		return n.ocrJob.status
	}
	if body != nil {
		return body.ocrStatus
	}
	if row.OcrStatus != nil {
		return *row.OcrStatus
	}
	return ""
}

func (n *nativeMain) eligibleOCR(row protocol.DesktopEntry, generation uint64) bool {
	return n.session != nil && generation == n.history.generation && row.Id > 0 &&
		row.ContentType == "image" && !n.failed && !n.mutating && !n.hasHistoryDialog() &&
		(!nativeConcealed(row) || n.canUseProtectedRow(row))
}

func (n *nativeMain) ocrParams(row protocol.DesktopEntry) protocol.ManualOcrParams {
	state := n.history.state
	return protocol.ManualOcrParams{
		Id: row.Id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, Reveal: nativeConcealed(row),
	}
}

func (n *nativeMain) recognizeOCR(row protocol.DesktopEntry, generation uint64) {
	if !n.eligibleOCR(row, generation) || n.private || n.ocrJob.status == "processing" ||
		n.ocrStatus(row, n.protectedBody(row)) == "processing" {
		return
	}
	n.closeOCRPreview()
	n.closePreview()
	n.ocrJob = nativeOCRJob{id: row.Id, status: "processing"}
	n.notice = ""
	session, params := n.session, n.ocrParams(row)
	go func() {
		ctx, cancel := context.WithTimeout(session, 70*time.Second)
		defer cancel()
		err := n.library.recognizeForNative(ctx, params)
		n.update(session, func() {
			if n.ocrJob.id != row.Id || n.ocrJob.status != "processing" {
				return
			}
			n.ocrJob = nativeOCRJob{}
			if err != nil {
				status := "failed"
				var remote *client.RemoteError
				if errors.As(err, &remote) && remote.Code == protocol.WireErrorCodeUnsupportedCapability {
					status = "unsupported"
				}
				n.ocrJob = nativeOCRJob{id: row.Id, status: status}
			}
		})
	}()
}

func (n *nativeMain) drawCardOCR(c *ui.Context, row protocol.DesktopEntry, body *nativeProtectedBody, generation uint64) {
	if row.Id <= 0 {
		return
	}
	status := n.ocrStatus(row, body)
	switch status {
	case "processing":
		ui.Text(c, n.text("ocr_processing")).FontSize(11).TextColor(c.Theme().TextMuted)
	case "failed":
		ui.Text(c, n.text("ocr_failed_label")).FontSize(11).TextColor(c.Theme().Danger)
	case "unsupported":
		ui.Text(c, n.text("ocr_unsupported_label")).FontSize(11).TextColor(c.Theme().TextMuted)
	case "done":
		n.drawOCRResult(c, row, generation)
	}
}

func (n *nativeMain) drawOCRResult(c *ui.Context, row protocol.DesktopEntry, generation uint64) {
	key := nativeCardKey{row.Id, row.Timestamp}
	open := n.ocrPreview != nil && n.ocrPreview.key == key
	label := n.text("ocr_done_label")
	arrow := "▸ "
	if open {
		arrow = "▾ "
	}
	toggle := ui.ButtonBase(c).Label(label).Tooltip(n.text("ocr_toggle_label")).AlignSelf(ui.Start).
		Height(22).PaddingX(4).Radius(c.Theme().Radius).FocusRing(true)
	if toggle.Hovered() {
		toggle.Background(c.Theme().SurfaceHover)
	}
	toggle.Children(func() { ui.Text(c, arrow+label).FontSize(11).TextColor(c.Theme().TextMuted) })
	if toggle.Clicked() {
		if open {
			n.closeOCRPreview()
			return
		}
		n.openOCRPreview(row, generation)
		return
	}
	if !open {
		return
	}
	form := n.ocrPreview
	switch {
	case form.loading:
		ui.Text(c, n.text("common_loading")).FontSize(11).TextColor(c.Theme().TextMuted)
	case form.failed:
		ui.Text(c, n.text("ocr_failed_label")).FontSize(11).TextColor(c.Theme().Danger)
	case strings.TrimSpace(form.text) == "":
		ui.Text(c, n.text("native_ocr_empty")).FontSize(11).TextColor(c.Theme().TextMuted)
	default:
		ui.Scroll(c).FillWidth().MaxHeight(80).Children(func() {
			ui.Text(c, form.text).FontSize(11).TextColor(c.Theme().TextMuted).Wrap()
		})
		if uikit.Button(c, uikit.ButtonOptions{Label: n.text("native_ocr_copy"), Disabled: n.mutating}).Clicked() {
			n.copyOCR(row, generation)
		}
	}
}

func (n *nativeMain) openOCRPreview(row protocol.DesktopEntry, generation uint64) {
	if !n.eligibleOCR(row, generation) {
		return
	}
	n.closeOCRPreview()
	session, params := n.session, n.ocrParams(row)
	ctx, cancel := context.WithTimeout(session, 4*time.Second)
	form := &nativeOCRPreview{key: nativeCardKey{row.Id, row.Timestamp}, cancel: cancel, loading: true}
	n.ocrPreview = form
	go func() {
		defer cancel()
		result, err := n.library.nativeOCRPreview(ctx, params)
		n.update(session, func() {
			if n.ocrPreview != form {
				return
			}
			form.loading = false
			form.failed = err != nil || result.Status != "done"
			if !form.failed && result.Text != nil {
				form.text = *result.Text
			}
		})
	}()
}

func (n *nativeMain) closeOCRPreview() {
	if form := n.ocrPreview; form != nil {
		form.cancel()
		form.text = ""
		n.ocrPreview = nil
	}
}

func (n *nativeMain) copyOCR(row protocol.DesktopEntry, generation uint64) {
	if !n.eligibleOCR(row, generation) || n.ocrStatus(row, n.protectedBody(row)) != "done" {
		return
	}
	n.mutating, n.notice = true, ""
	session, params := n.session, n.ocrParams(row)
	go func() {
		ctx, cancel := context.WithTimeout(session, 5*time.Second)
		defer cancel()
		err := n.library.copyOCRText(ctx, params)
		n.update(session, func() {
			n.mutating = false
			if err != nil {
				n.notice = n.text("native_ocr_copy_failed")
				return
			}
			n.notice = n.text("tag_copied")
		})
	}()
}

func (n *nativeMain) drawOCREngine(c *ui.Context) {
	if n.ocrEngine == nil {
		if n.session != nil && !n.ocrEngineLoading {
			n.ocrEngineLoading = true
			session := n.session
			go func() {
				ctx, cancel := context.WithTimeout(session, 7*time.Second)
				defer cancel()
				engine, _ := n.library.OcrEngine(ctx)
				n.update(session, func() {
					n.ocrEngine, n.ocrEngineLoading = &engine, false
				})
			}()
		}
		return
	}
	status := n.text("ocr_engine_unavailable")
	if n.ocrEngine.Available {
		status = n.text("ocr_engine_available")
	}
	name := n.ocrEngine.EngineName
	if name != "" {
		status = name + " · " + status
	}
	ui.Text(c, n.text("ocr_status_label")+" · "+status).TextColor(c.Theme().TextMuted).Wrap()
}
