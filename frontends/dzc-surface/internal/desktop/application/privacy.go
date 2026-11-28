package application

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
	desktopruntime "github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/desktop/runtime"
)

func PrivacyStatus(ctx context.Context, owner *client.Client, observe func(error)) (protocol.ServiceStatus, error) {
	return readServiceStatus(ctx, owner.Hello().State.ServiceInstanceId, func(result *protocol.ServiceStatus) error {
		return owner.DesktopCall(ctx, protocol.MethodServiceStatus, struct{}{}, "ServiceStatus", result)
	}, observe)
}

func readServiceStatus(ctx context.Context, instance string, read func(*protocol.ServiceStatus) error, observe func(error)) (protocol.ServiceStatus, error) {
	var status protocol.ServiceStatus
	err := readPrivacyStatus(ctx, func() error {
		status = protocol.ServiceStatus{}
		err := read(&status)
		if err == nil && !matchesPrivacyState(instance, status.State, status.Privacy) {
			err = client.ErrProtocol
		}
		if observe != nil {
			observe(err)
		}
		return err
	})
	if err != nil {
		return protocol.ServiceStatus{}, err
	}
	return status, nil
}

func matchesPrivacyState(instance string, state protocol.RuntimeState, privacy protocol.PrivacyState) bool {
	return state.ServiceInstanceId == instance && state.PrivacyEpoch == privacy.PrivacyEpoch
}

func retryablePrivacyStatus(err error) bool {
	var remote *client.RemoteError
	if errors.As(err, &remote) {
		return remote.Code == protocol.WireErrorCodeBusy
	}
	return desktopruntime.EventTransportFailure(err)
}

// ServiceStatus rejects a snapshot if history changes while it is assembled.
// Retry only this read, on fresh authenticated connections, never a mutation.
func readPrivacyStatus(ctx context.Context, read func() error) error {
	for attempt, delay := range []time.Duration{0, 25 * time.Millisecond, 75 * time.Millisecond, 200 * time.Millisecond} {
		if delay > 0 {
			timer := time.NewTimer(delay)
			select {
			case <-ctx.Done():
				timer.Stop()
				return ctx.Err()
			case <-timer.C:
			}
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		err := read()
		if err == nil || ctx.Err() != nil || !retryablePrivacyStatus(err) {
			return err
		}
		if attempt == 3 {
			return fmt.Errorf("PrivacyStatusRetryable: %s", desktopruntime.DiagnosticError(err))
		}
	}
	return nil
}

func SetPrivacy(ctx context.Context, owner *client.Client, params protocol.PrivacySetParams) (protocol.ServiceStatus, error) {
	if !params.Active && params.DurationMs != nil {
		return protocol.ServiceStatus{}, errors.New("PrivacyDurationInvalid")
	}
	var privacy protocol.PrivacyState
	if err := owner.DesktopCall(ctx, protocol.MethodPrivacySet, params, "PrivacyState", &privacy); err != nil {
		return protocol.ServiceStatus{}, err
	}
	status, err := PrivacyStatus(ctx, owner, nil)
	if err == nil && status.Privacy.PrivacyEpoch != privacy.PrivacyEpoch {
		return protocol.ServiceStatus{}, errors.New("PrivacyEpochChanged")
	}
	return status, err
}

func SetPrivacyHotkey(ctx context.Context, owner *client.Client, hotkey string) (string, error) {
	var result protocol.PrivacyHotkey
	err := owner.DesktopCall(ctx, protocol.MethodPrivacyHotkeySet, protocol.PrivacyHotkey{Hotkey: hotkey}, "PrivacyHotkey", &result)
	var remote *client.RemoteError
	if errors.As(err, &remote) {
		switch remote.Code {
		case protocol.WireErrorCodeBusy:
			return "", errors.New("PrivacyHotkeyBusy")
		case protocol.WireErrorCodeUnsupportedCapability:
			return "", errors.New("PrivacyHotkeyUnavailable")
		}
	}
	return result.Hotkey, err
}

func (r LibraryReader) PolicyRules(ctx context.Context, epoch uint64) ([]protocol.PolicyRuleValue, error) {
	params := protocol.DesktopPolicyParams{ServiceInstanceId: r.owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch}
	var result protocol.DesktopPolicyRules
	_, err := r.ReadSnapshot(ctx, protocol.MethodDesktopPrivacyRules, params, "DesktopPolicyRules", &result, epoch)
	return result.Rules, err
}

func PolicyDecision(ctx context.Context, owner *client.Client, params protocol.DesktopPolicyParams) (protocol.DesktopPolicyDecision, error) {
	var result protocol.DesktopPolicyDecision
	if params.ServiceInstanceId != owner.Hello().State.ServiceInstanceId {
		return result, client.ErrProtocol
	}
	err := owner.DesktopCall(ctx, protocol.MethodDesktopPrivacyDecision, params, "DesktopPolicyDecision", &result)
	return result, err
}

func PolicyFeedback(ctx context.Context, owner *client.Client, params protocol.DesktopPolicyParams) (protocol.DesktopRuntimeValue, error) {
	return mutatePolicy(ctx, owner, protocol.MethodDesktopPrivacyFeedback, params)
}

func RemovePolicyRule(ctx context.Context, owner *client.Client, id string, epoch uint64) (protocol.DesktopRuntimeValue, error) {
	params := protocol.DesktopPolicyParams{ServiceInstanceId: owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, RuleId: &id}
	return mutatePolicy(ctx, owner, protocol.MethodDesktopPrivacyRemoveRule, params)
}

func mutatePolicy(ctx context.Context, owner *client.Client, method protocol.Method, params protocol.DesktopPolicyParams) (protocol.DesktopRuntimeValue, error) {
	if params.ServiceInstanceId != owner.Hello().State.ServiceInstanceId {
		return protocol.DesktopRuntimeValue{}, client.ErrProtocol
	}
	var result protocol.DesktopRuntimeValue
	if err := owner.DesktopCall(ctx, method, params, "DesktopRuntimeValue", &result); err != nil {
		return protocol.DesktopRuntimeValue{}, err
	}
	if !matchesPrivacyState(params.ServiceInstanceId, result.State, result.Privacy) {
		return protocol.DesktopRuntimeValue{}, client.ErrProtocol
	}
	return result, nil
}
