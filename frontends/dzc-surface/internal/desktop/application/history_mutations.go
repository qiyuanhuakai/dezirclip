package application

import (
	"context"
	"encoding/base64"
	"errors"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

// MutateEntry uses the identity and privacy epoch of the row the user acted on.
func MutateEntry(ctx context.Context, owner *client.Client, params protocol.DesktopMutationParams) (int64, error) {
	if params.ServiceInstanceId != owner.Hello().State.ServiceInstanceId || params.Id == 0 {
		return 0, client.ErrProtocol
	}
	switch params.Action {
	case "pin", "tags", "edit", "delete":
	default:
		return 0, client.ErrProtocol
	}
	var result protocol.DesktopMutationResult
	err := owner.DesktopCall(ctx, protocol.MethodDesktopHistoryMutate, params, "DesktopMutationResult", &result)
	if err == nil && (!result.Performed || result.Id == 0) {
		return 0, client.ErrProtocol
	}
	return result.Id, err
}

func AddManual(ctx context.Context, owner *client.Client, content string, params protocol.ManualEntryParams) (int64, error) {
	if len(content) > 32*1024*1024 {
		return 0, errors.New("ManualEntryTooLarge")
	}
	params.ServiceInstanceId = owner.Hello().State.ServiceInstanceId
	host, err := owner.DesktopConnection(ctx)
	if err != nil {
		return 0, err
	}
	defer host.Close()
	var begin protocol.CaptureBeginResult
	if err := host.Call(ctx, protocol.MethodDesktopHistoryManualBegin, params, "CaptureBeginResult", &begin); err != nil {
		return 0, err
	}
	if begin.CaptureId == nil || begin.State.ServiceInstanceId != params.ServiceInstanceId || begin.State.PrivacyEpoch != params.ExpectedEpoch {
		return 0, client.ErrProtocol
	}
	const chunkSize = 192 * 1024
	for offset := 0; offset < len(content); offset += chunkSize {
		chunk := content[offset:min(offset+chunkSize, len(content))]
		write := protocol.CaptureWriteParams{CaptureId: *begin.CaptureId, Offset: uint32(offset), DataBase64: base64.StdEncoding.EncodeToString([]byte(chunk))}
		var result protocol.OperationResult
		if err := host.Call(ctx, protocol.MethodDesktopCaptureWrite, write, "OperationResult", &result); err != nil {
			return 0, err
		}
		if !result.Performed {
			return 0, client.ErrProtocol
		}
	}
	var saved protocol.DesktopMutationResult
	if err := host.Call(ctx, protocol.MethodDesktopHistoryManualFinish, protocol.ManualEntryFinish{CaptureId: *begin.CaptureId}, "DesktopMutationResult", &saved); err != nil {
		return 0, err
	}
	if !saved.Performed || saved.Id <= 0 {
		return 0, client.ErrProtocol
	}
	return saved.Id, nil
}
