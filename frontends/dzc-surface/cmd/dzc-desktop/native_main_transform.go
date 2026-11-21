package main

import (
	"context"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

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
	protected := nativeConcealed(row)
	if !n.canOpenHistoryDialog() || n.history.generation != generation || !nativeTextToolEntry(row.ContentType) ||
		protected && !n.canUseProtectedRow(row) {
		return
	}
	n.mutating, n.notice = true, ""
	session, state := n.session, n.history.state
	lifetime := session
	if protected {
		lifetime = n.protectedRows[nativeCardKey{row.Id, row.Timestamp}].ctx
	}
	params := protocol.DesktopTransformCopyParams{Id: row.Id, ServiceInstanceId: state.ServiceInstanceId,
		ExpectedEpoch: state.PrivacyEpoch, Kind: kind, Reveal: protected}
	go func() {
		ctx, cancel := context.WithTimeout(lifetime, 5*time.Second)
		defer cancel()
		err := n.library.CopyTransformed(ctx, params)
		n.update(session, func() {
			n.mutating = false
			if lifetime.Err() != nil {
				return
			}
			if err != nil {
				n.notice = n.text("native_transform_failed")
			} else {
				n.notice = n.text("transform_applied")
			}
		})
	}()
}
