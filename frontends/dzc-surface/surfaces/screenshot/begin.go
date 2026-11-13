package screenshot

import (
	"context"
	"errors"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

// An explicit Busy means no screen session was accepted. Retry only that
// rejection, never a lost response or finish (which may have copied already).
func retryBusyBegin(ctx context.Context, begin func() error) error {
	for attempt := 0; ; attempt++ {
		if err := ctx.Err(); err != nil {
			return err
		}
		err := begin()
		var remote *client.RemoteError
		if attempt == 4 || !errors.As(err, &remote) || remote.Code != protocol.WireErrorCodeBusy {
			return err
		}
		timer := time.NewTimer(25 * time.Millisecond)
		select {
		case <-ctx.Done():
			timer.Stop()
			return ctx.Err()
		case <-timer.C:
		}
	}
}
