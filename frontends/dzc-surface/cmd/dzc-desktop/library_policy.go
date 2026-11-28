package main

import (
	"context"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	"github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/desktop/application"
)

func (l *Library) PolicyRules(ctx context.Context, epoch uint64) ([]protocol.PolicyRuleValue, error) {
	return l.reader().PolicyRules(ctx, epoch)
}

func (l *Library) PolicyDecision(ctx context.Context, params protocol.DesktopPolicyParams) (protocol.DesktopPolicyDecision, error) {
	return application.PolicyDecision(ctx, l.owner, params)
}

func (l *Library) PolicyFeedback(ctx context.Context, params protocol.DesktopPolicyParams) (protocol.DesktopRuntimeValue, error) {
	return application.PolicyFeedback(ctx, l.owner, params)
}

func (l *Library) RemovePolicyRule(ctx context.Context, id string, epoch uint64) (protocol.DesktopRuntimeValue, error) {
	return application.RemovePolicyRule(ctx, l.owner, id, epoch)
}
