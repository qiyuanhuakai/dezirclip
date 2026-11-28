package runtime

import (
	"context"
	"errors"
	"io"
	"net"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
)

// DiagnosticError classifies an error without exposing its message or request data.
func DiagnosticError(err error) string {
	if err == nil {
		return ""
	}
	var remote *client.RemoteError
	if errors.As(err, &remote) {
		switch string(remote.Code) {
		case "busy", "unavailable", "privacy_blocked", "privacy_epoch_changed", "permission_denied", "session_expired":
			return "remote-" + string(remote.Code)
		default:
			return "remote"
		}
	}
	if errors.Is(err, client.ErrProtocol) {
		return "protocol"
	}
	if errors.Is(err, context.Canceled) {
		return "cancelled"
	}
	if errors.Is(err, context.DeadlineExceeded) {
		return "deadline"
	}
	if errors.Is(err, io.EOF) || errors.Is(err, io.ErrUnexpectedEOF) {
		return "eof"
	}
	var timeout net.Error
	if errors.As(err, &timeout) && timeout.Timeout() {
		return "transport-timeout"
	}
	if EventTransportFailure(err) {
		return "transport"
	}
	return "other"
}
