package main

import (
	"context"
	"errors"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestSnapshotReadsRetryOnlyExplicitBusy(t *testing.T) {
	calls := 0
	err := readLibrarySnapshot(context.Background(), func(context.Context) error {
		calls++
		if calls < 3 {
			return &client.RemoteError{Code: protocol.WireErrorCodeBusy}
		}
		return nil
	})
	if err != nil || calls != 3 {
		t.Fatalf("busy read: calls=%d err=%v", calls, err)
	}
	for _, failure := range []error{client.ErrProtocol, context.DeadlineExceeded, &client.RemoteError{Code: protocol.WireErrorCodePrivacyBlocked}} {
		calls = 0
		err = readLibrarySnapshot(context.Background(), func(context.Context) error { calls++; return failure })
		if calls != 1 || err != failure {
			t.Fatalf("replayed non-Busy failure: calls=%d err=%v", calls, err)
		}
	}
}

func TestSnapshotReadAdmissionIsBoundedAndCancellable(t *testing.T) {
	entered := make(chan struct{}, 8)
	release := make(chan struct{})
	var active, peak atomic.Int32
	var workers sync.WaitGroup
	for range 8 {
		workers.Go(func() {
			if err := readLibrarySnapshot(context.Background(), func(context.Context) error {
				n := active.Add(1)
				for old := peak.Load(); n > old && !peak.CompareAndSwap(old, n); old = peak.Load() {
				}
				entered <- struct{}{}
				<-release
				active.Add(-1)
				return nil
			}); err != nil {
				t.Error(err)
			}
		})
	}
	<-entered
	<-entered
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	err := readLibrarySnapshot(ctx, func(context.Context) error { t.Error("cancelled queued read ran"); return nil })
	close(release)
	workers.Wait()
	if !errors.Is(err, context.DeadlineExceeded) || peak.Load() != 2 {
		t.Fatalf("err=%v peak=%d", err, peak.Load())
	}
}
