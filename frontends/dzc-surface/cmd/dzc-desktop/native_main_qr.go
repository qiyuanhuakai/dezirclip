package main

import (
	"context"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type nativeQRDialog struct {
	ctx     context.Context
	cancel  context.CancelFunc
	request protocol.DesktopQrCopyParams
	open    bool
	loading bool
	image   ui.ImageSource
	summary string
	message string
	failed  bool
}

func (n *nativeMain) openQR(row protocol.DesktopEntry, generation uint64) {
	if !n.canOpenHistoryDialog() || generation != n.history.generation || !nativeTextToolEntry(row.ContentType) {
		return
	}
	reveal := nativeConcealed(row)
	if reveal && !n.canUseProtectedRow(row) {
		return
	}
	n.resetPreviewSelection()
	n.notice = ""
	session, state := n.session, n.history.state
	lifetime, stop := context.WithCancel(session)
	form := &nativeQRDialog{
		ctx: lifetime, cancel: stop, open: true, loading: true,
		request: protocol.DesktopQrCopyParams{
			Id: row.Id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, Reveal: reveal,
		},
	}
	n.qr = form
	params := protocol.DesktopEntryParams{
		Id: row.Id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, Reveal: reveal,
	}
	go func() {
		ctx, cancel := context.WithTimeout(lifetime, 4*time.Second)
		defer cancel()
		preview, err := n.library.historyQR(ctx, params)
		var image ui.ImageSource
		if err == nil && ctx.Err() == nil {
			image = previewSourceImage(preview.Image)
		}
		n.update(session, func() {
			if n.qr != form {
				return
			}
			form.loading = false
			if image == nil {
				form.message, form.failed = n.text("native_qr_unavailable"), true
				return
			}
			form.image, form.summary, form.request.ContentSha256 = image, preview.Summary, preview.ContentSha256
		})
	}()
}

func (n *nativeMain) qrView(c *ui.Context, kit uikit.Kit, form *nativeQRDialog) {
	kit.Dialog(c, &form.open, n.text("native_qr_code"), "", n.mutating, func() {
		if form.loading {
			ui.Text(c, n.text("common_loading"))
		}
		if form.image != nil {
			width, height := c.Size()
			size := max(48, min(256, width-72, height-175))
			ui.Box(c).FillWidth().Height(size).Center().Children(func() {
				ui.Image(c, form.image).Size(size, size).Fit(ui.Contain).Label(n.text("native_qr_code"))
			})
			ui.Text(c, form.summary).TextColor(c.Theme().TextMuted).Wrap()
		}
		if form.message != "" {
			color := c.Theme().TextMuted
			if form.failed {
				color = c.Theme().Danger
			}
			ui.Text(c, form.message).TextColor(color).Wrap()
		}
		ui.Row(c).Gap(6).Wrap().Justify(ui.End).Children(func() {
			if n.mainKit().Button(c, uikit.ButtonOptions{Label: n.text("common_close"), Disabled: n.mutating}).Clicked() {
				form.open = false
			}
			if n.mainKit().Button(c, uikit.ButtonOptions{
				Label: n.text("native_qr_copy_svg"), Primary: true, Disabled: form.image == nil || n.mutating,
			}).Clicked() {
				n.copyQR(form)
			}
		})
	})
	if !form.open {
		n.closeQR()
	}
}

func (n *nativeMain) copyQR(form *nativeQRDialog) {
	if n.session == nil || n.qr != form || form.image == nil || n.mutating ||
		form.request.Reveal && !mainWindowFocused(n.window) {
		return
	}
	session, request := n.session, form.request
	n.mutating = true
	form.message, form.failed = "", false
	go func() {
		ctx, cancel := context.WithTimeout(form.ctx, 5*time.Second)
		defer cancel()
		err := n.library.copyHistoryQR(ctx, request)
		n.update(session, func() {
			n.mutating = false
			if n.qr != form {
				return
			}
			if err != nil {
				form.message, form.failed = n.text("native_qr_copy_failed"), true
				return
			}
			form.message = n.text("tag_copied")
		})
	}()
}

func (n *nativeMain) closeQR() {
	if form := n.qr; form != nil {
		form.cancel()
		form.image, form.summary, form.request.ContentSha256 = nil, "", ""
		n.qr = nil
	}
}
