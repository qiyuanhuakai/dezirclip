package main

import (
	"context"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/packages/ui-native/uikit"
)

type nativeTransformDialog struct {
	ctx             context.Context
	cancel          context.CancelFunc
	request         protocol.DesktopTransformCopyParams
	open, protected bool
	loading         bool
	message         string
}

func nativeTextToolEntry(kind string) bool {
	switch kind {
	case "", "image", "video", "file", "files":
		return false
	default:
		return true
	}
}

func (n *nativeMain) loadMenuTransforms() {
	if n.session == nil || n.menuTransformsLoading || len(n.transformKinds) > 0 {
		return
	}
	n.menuTransformsLoading = true
	session := n.session
	go func() {
		ctx, cancel := context.WithTimeout(session, 4*time.Second)
		defer cancel()
		catalog, err := n.library.ToolCatalog(ctx)
		n.update(session, func() {
			n.menuTransformsLoading = false
			if err == nil {
				n.transformKinds = catalog.Transforms
			}
		})
	}()
}

func (n *nativeMain) copyMenuTransform(row protocol.DesktopEntry, generation uint64, kind string) {
	if !n.canOpenHistoryDialog() || n.history.generation != generation || nativeConcealed(row) || !nativeTextToolEntry(row.ContentType) {
		return
	}
	n.mutating, n.notice = true, ""
	session, state := n.session, n.history.state
	params := protocol.DesktopTransformCopyParams{Id: row.Id, ServiceInstanceId: state.ServiceInstanceId,
		ExpectedEpoch: state.PrivacyEpoch, Kind: kind}
	go func() {
		ctx, cancel := context.WithTimeout(session, 5*time.Second)
		defer cancel()
		err := n.library.CopyTransformed(ctx, params)
		n.update(session, func() {
			n.mutating = false
			if err != nil {
				n.notice = n.text("native_transform_failed")
			} else {
				n.notice = n.text("transform_applied")
			}
		})
	}()
}

func (n *nativeMain) openTransform(row protocol.DesktopEntry, generation uint64) {
	if !n.canOpenHistoryDialog() || n.history.generation != generation || !nativeTextToolEntry(row.ContentType) {
		return
	}
	protected := nativeConcealed(row)
	if protected && !n.canUseProtectedRow(row) {
		return
	}
	n.resetPreviewSelection()
	session := n.session
	state := n.history.state
	lifetime, stop := context.WithCancel(session)
	form := &nativeTransformDialog{
		ctx: lifetime, cancel: stop, open: true, protected: protected,
		request: protocol.DesktopTransformCopyParams{
			Id: row.Id, ServiceInstanceId: state.ServiceInstanceId, ExpectedEpoch: state.PrivacyEpoch, Reveal: protected,
		},
	}
	n.transform = form
	n.notice = ""
	if len(n.transformKinds) != 0 {
		return
	}
	form.loading = true
	go func() {
		ctx, cancel := context.WithTimeout(lifetime, 4*time.Second)
		defer cancel()
		catalog, err := n.library.ToolCatalog(ctx)
		n.update(session, func() {
			if n.transform != form {
				return
			}
			form.loading = false
			if err != nil || len(catalog.Transforms) == 0 {
				form.message = n.text("native_transform_unavailable")
				return
			}
			n.transformKinds = catalog.Transforms
		})
	}()
}

func (n *nativeMain) transformView(c *ui.Context, kit uikit.Kit, form *nativeTransformDialog) {
	kit.Dialog(c, &form.open, n.text("native_transformations"), n.text("common_close"), n.mutating, func() {
		if form.loading {
			ui.Text(c, n.text("common_loading"))
		}
		if form.message != "" {
			ui.Text(c, form.message).TextColor(c.Theme().Danger).Wrap()
		}
		_, height := c.Size()
		ui.Scroll(c).FillWidth().MaxHeight(max(72, height-146)).Children(func() {
			ui.Column(c).FillWidth().Gap(4).Children(func() {
				for _, kind := range n.transformKinds {
					label := kind.LabelZh
					if n.language() == "en" {
						label = kind.LabelEn
					}
					if n.mainKit().Button(c, uikit.ButtonOptions{Label: label, Disabled: n.mutating}).Clicked() {
						n.copyTransformed(form, kind.Id)
					}
				}
			})
		})
	})
	if !form.open {
		n.closeTransform()
	}
}

func (n *nativeMain) closeTransform() {
	if form := n.transform; form != nil {
		form.cancel()
		n.transform = nil
	}
}

func (n *nativeMain) copyTransformed(form *nativeTransformDialog, kind string) {
	if n.session == nil || n.transform != form || n.mutating || form.protected && !mainWindowFocused(n.window) {
		return
	}
	session, lifetime := n.session, form.ctx
	params := form.request
	params.Kind = kind
	n.mutating, form.message = true, ""
	go func() {
		ctx, cancel := context.WithTimeout(lifetime, 5*time.Second)
		defer cancel()
		err := n.library.CopyTransformed(ctx, params)
		n.update(session, func() {
			n.mutating = false
			if n.transform != form {
				return
			}
			if err != nil {
				form.message = n.text("native_transform_failed")
				return
			}
			n.closeTransform()
			n.notice = n.text("transform_applied")
		})
	}()
}
