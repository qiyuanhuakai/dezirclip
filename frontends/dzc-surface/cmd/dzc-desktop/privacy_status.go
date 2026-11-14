package main

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func retryablePrivacyStatus(err error) bool {
	var remote *client.RemoteError
	if errors.As(err, &remote) {
		return remote.Code == protocol.WireErrorCodeBusy
	}
	return eventTransportFailure(err)
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
			return fmt.Errorf("PrivacyStatusRetryable: %s", diagnosticError(err))
		}
	}
	return nil
}

func (h *History) PrivacyStatus(ctx context.Context) (protocol.ServiceStatus, error) {
	return (&Library{h.actions}).readStatus(ctx, h.diagnostics.statusRead)
}
