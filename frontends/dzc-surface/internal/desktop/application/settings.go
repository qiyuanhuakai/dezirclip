package application

import (
	"context"
	"errors"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func SetSetting(ctx context.Context, host *client.Client, key, value string, epoch uint64) error {
	instance := host.Hello().State.ServiceInstanceId
	method := protocol.DesktopSettingMethod(key)
	var params any
	switch method {
	case protocol.MethodDesktopApplicationsSelect:
		params = protocol.ApplicationSelection{ServiceInstanceId: instance, ExpectedEpoch: epoch, Key: key, Value: value}
	case protocol.MethodDesktopSettingsViewSet:
		params = protocol.ViewSettingParams{ServiceInstanceId: instance, ExpectedEpoch: epoch, Key: key, Value: value}
	case protocol.MethodDesktopQueueConfigure:
		params = protocol.QueueSettingParams{ServiceInstanceId: instance, ExpectedEpoch: epoch, Key: key, Value: value}
	case protocol.MethodDesktopSettingsCaptureSet:
		params = protocol.CaptureSettingParams{ServiceInstanceId: instance, ExpectedEpoch: epoch, Key: key, Value: value}
	default:
		return errors.New("UnsupportedDesktopSetting")
	}
	var result protocol.OperationResult
	if err := host.DesktopCall(ctx, method, params, "OperationResult", &result); err != nil {
		return err
	}
	if !result.Performed {
		return client.ErrProtocol
	}
	return nil
}
