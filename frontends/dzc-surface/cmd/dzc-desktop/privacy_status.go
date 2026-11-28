package main

import (
	"context"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/desktop/application"
)

func (h *History) PrivacyStatus(ctx context.Context) (protocol.ServiceStatus, error) {
	return application.PrivacyStatus(ctx, h.actions, h.diagnostics.statusRead)
}
