package main

import (
	"context"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func (l *Library) PolicyRules(ctx context.Context, epoch uint64) ([]protocol.PolicyRuleValue, error) {
	params := protocol.DesktopPolicyParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch}
	var result protocol.DesktopPolicyRules
	err := l.snapshot(ctx, protocol.MethodDesktopPrivacyRules, params, "DesktopPolicyRules", &result, epoch)
	return result.Rules, err
}

func (l *Library) PolicyDecision(ctx context.Context, params protocol.DesktopPolicyParams) (protocol.DesktopPolicyDecision, error) {
	var result protocol.DesktopPolicyDecision
	if params.ServiceInstanceId != l.owner.Hello().State.ServiceInstanceId {
		return result, client.ErrProtocol
	}
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopPrivacyDecision, params, "DesktopPolicyDecision", &result)
	return result, err
}

func (l *Library) PolicyFeedback(ctx context.Context, params protocol.DesktopPolicyParams) (protocol.DesktopRuntimeValue, error) {
	var result protocol.DesktopRuntimeValue
	if params.ServiceInstanceId != l.owner.Hello().State.ServiceInstanceId {
		return result, client.ErrProtocol
	}
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopPrivacyFeedback, params, "DesktopRuntimeValue", &result)
	return result, err
}

func (l *Library) RemovePolicyRule(ctx context.Context, id string, epoch uint64) (protocol.DesktopRuntimeValue, error) {
	var result protocol.DesktopRuntimeValue
	params := protocol.DesktopPolicyParams{ServiceInstanceId: l.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, RuleId: &id}
	err := l.owner.DesktopCall(ctx, protocol.MethodDesktopPrivacyRemoveRule, params, "DesktopRuntimeValue", &result)
	return result, err
}
