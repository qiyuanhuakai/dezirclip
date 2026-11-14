package main

import (
	"context"
	"errors"
	"io"
	"net"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
)

// Event reads are cursor-based and have no clipboard side effects. An idle
// pipe can expire while Chromium throttles an occluded page during capture.
// Only that transport is replaced; protocol and privacy failures stay visible.
func readEventStream(ctx context.Context, read func(fresh bool) error) error {
	err := read(false)
	if err == nil || ctx.Err() != nil || !eventTransportFailure(err) {
		return err
	}
	return read(true)
}

func eventTransportFailure(err error) bool {
	var remote *client.RemoteError
	if errors.As(err, &remote) || errors.Is(err, client.ErrProtocol) || errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
		return false
	}
	var network *net.OpError
	return errors.As(err, &network) || errors.Is(err, io.EOF) || errors.Is(err, io.ErrUnexpectedEOF) ||
		errors.Is(err, io.ErrClosedPipe) || errors.Is(err, net.ErrClosed)
}
