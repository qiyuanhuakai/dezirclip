package main

import (
	"context"
	"errors"
	"io"
	"testing"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

type settingsTestConnection struct {
	err    error
	closed bool
	calls  int
}

func (c *settingsTestConnection) Call(context.Context, protocol.Method, any, string, any) error {
	c.calls++
	return c.err
}
func (c *settingsTestConnection) Close() error { c.closed = true; return nil }

func TestWinVDisconnectedPollingReleasesPending(t *testing.T) {
	host := &settingsTestConnection{}
	change := &systemChange{host: host, token: "old", stop: make(chan struct{}), done: make(chan struct{})}
	close(change.done)
	s := &SystemSettings{pending: change}
	_, err := s.FinishWinV(context.Background(), "old", true)
	if err == nil || s.pending != nil || !host.closed || host.calls != 0 {
		t.Fatal("disconnected receipt was reused or retained")
	}
}

func TestWinVFinishRetainsOnlyRetryableRemoteErrors(t *testing.T) {
	for _, remote := range []bool{false, true} {
		host := &settingsTestConnection{err: io.EOF}
		if remote {
			host.err = &client.RemoteError{}
		}
		change := &systemChange{host: host, token: "receipt", stop: make(chan struct{}), done: make(chan struct{})}
		go func() { <-change.stop; close(change.done) }()
		s := &SystemSettings{pending: change}
		finished := make(chan error, 1)
		go func() { _, err := s.FinishWinV(context.Background(), "receipt", true); finished <- err }()
		select {
		case err := <-finished:
			if !errors.Is(err, host.err) {
				t.Fatal(err)
			}
		case <-time.After(time.Second):
			t.Fatal("finish deadlocked with polling shutdown")
		}
		if remote {
			if s.pending != change || host.closed {
				t.Fatal("retryable receipt discarded")
			}
			host.err = nil
			if _, err := s.FinishWinV(context.Background(), "receipt", true); err != nil {
				t.Fatal(err)
			}
		}
		if s.pending != nil || !host.closed {
			t.Fatal("finished or disconnected receipt retained")
		}
	}
}
