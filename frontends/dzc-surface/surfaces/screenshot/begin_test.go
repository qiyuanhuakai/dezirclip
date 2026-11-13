package screenshot

import (
	"context"
	"errors"
	"io"
	"testing"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestScreenBeginRetriesOnlyExplicitBusy(t *testing.T) {
	busy := &client.RemoteError{Code: protocol.WireErrorCodeBusy}
	calls := 0
	if err := retryBusyBegin(context.Background(), func() error {
		calls++
		if calls < 3 {
			return busy
		}
		return nil
	}); err != nil || calls != 3 {
		t.Fatalf("busy then success: %d %v", calls, err)
	}
	for _, failure := range []error{io.EOF, &client.RemoteError{Code: protocol.WireErrorCodePrivacyEpochChanged}} {
		calls = 0
		err := retryBusyBegin(context.Background(), func() error { calls++; return failure })
		if err != failure || calls != 1 {
			t.Fatalf("replayed uncertain or unauthorized begin: %d %v", calls, err)
		}
	}
	calls = 0
	err := retryBusyBegin(context.Background(), func() error { calls++; return busy })
	if err != busy || calls != 5 {
		t.Fatalf("unbounded busy retry: %d %v", calls, err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	calls = 0
	err = retryBusyBegin(ctx, func() error { calls++; cancel(); return busy })
	if !errors.Is(err, context.Canceled) || calls != 1 {
		t.Fatalf("cancelled begin replayed: %d %v", calls, err)
	}
}
