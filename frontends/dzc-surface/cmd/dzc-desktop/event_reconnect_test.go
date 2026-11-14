package main

import (
	"context"
	"errors"
	"io"
	"net"
	"testing"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestEventReadReconnectsOnceOnlyForTransportFailure(t *testing.T) {
	for _, failure := range []error{io.EOF, io.ErrClosedPipe, &net.OpError{Op: "read", Net: "pipe", Err: io.EOF}} {
		calls := 0
		err := readEventStream(t.Context(), func(fresh bool) error {
			calls++
			if fresh != (calls == 2) {
				t.Fatal("wrong reconnection flag")
			}
			if calls == 1 {
				return failure
			}
			return nil
		})
		if err != nil || calls != 2 {
			t.Fatalf("calls=%d error=%v", calls, err)
		}
	}
	calls := 0
	err := readEventStream(t.Context(), func(bool) error { calls++; return io.EOF })
	if !errors.Is(err, io.EOF) || calls != 2 {
		t.Fatal("reconnect must be bounded")
	}
}

func TestEventReadNeverRetriesPrivacyProtocolOrCancellation(t *testing.T) {
	for _, failure := range []error{client.ErrProtocol, context.Canceled, context.DeadlineExceeded,
		&client.RemoteError{Code: protocol.WireErrorCodePrivacyBlocked},
		&client.RemoteError{Code: protocol.WireErrorCodePrivacyEpochChanged},
		&client.RemoteError{Code: protocol.WireErrorCodeBusy}} {
		calls := 0
		err := readEventStream(t.Context(), func(bool) error { calls++; return failure })
		if err != failure || calls != 1 {
			t.Fatalf("replayed rejected read: %v", failure)
		}
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	calls := 0
	_ = readEventStream(ctx, func(bool) error { calls++; return io.EOF })
	if calls != 1 {
		t.Fatal("cancelled read reconnected")
	}
}
