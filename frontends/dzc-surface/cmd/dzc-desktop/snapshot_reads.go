package main

import (
	"context"
	"errors"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

// Leave room for the resident's snapshots in the same bounded Rust Host.
var librarySnapshotSlots = make(chan struct{}, 2)

func readLibrarySnapshot(ctx context.Context, read func(context.Context) error) error {
	select {
	case librarySnapshotSlots <- struct{}{}:
		defer func() { <-librarySnapshotSlots }()
	case <-ctx.Done():
		return ctx.Err()
	}
	deadline := time.Now().Add(2 * time.Second)
	for delay := 20 * time.Millisecond; ; delay = min(delay*2, 160*time.Millisecond) {
		if err := ctx.Err(); err != nil {
			return err
		}
		err := read(ctx)
		var remote *client.RemoteError
		if !errors.As(err, &remote) || remote.Code != protocol.WireErrorCodeBusy || time.Now().Add(delay).After(deadline) {
			return err
		}
		// Only the read-only snapshot path retries an explicit Host Busy response.
		timer := time.NewTimer(delay)
		select {
		case <-ctx.Done():
			timer.Stop()
			return ctx.Err()
		case <-timer.C:
		}
	}
}
