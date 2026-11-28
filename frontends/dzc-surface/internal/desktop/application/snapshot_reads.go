package application

import (
	"context"
	"errors"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

// SnapshotMeasurement records timings and counts for an optional desktop
// observer. It contains no response values or resource identifiers.
type SnapshotMeasurement struct {
	Method                       protocol.Method
	Bytes                        uint32
	Elapsed, Connect, Descriptor time.Duration
	Metrics                      client.SnapshotReadMetrics
	Complete                     bool
}

// LibraryReader keeps desktop snapshots on a separate authenticated stream.
type LibraryReader struct {
	owner   *client.Client
	observe func(SnapshotMeasurement)
}

func NewLibraryReader(owner *client.Client, observe func(SnapshotMeasurement)) LibraryReader {
	return LibraryReader{owner: owner, observe: observe}
}

func (r LibraryReader) ReadSnapshot(ctx context.Context, method protocol.Method, params any, schema string, result any, epoch uint64) (protocol.RuntimeState, error) {
	var state protocol.RuntimeState
	err := readLibrarySnapshot(ctx, func(ctx context.Context) error {
		var err error
		state, err = r.ReadSnapshotOnce(ctx, method, params, schema, result, epoch)
		return err
	})
	return state, err
}

// ReadSnapshotOnce is available to operations with their own admission budget.
func (r LibraryReader) ReadSnapshotOnce(ctx context.Context, method protocol.Method, params any, schema string, result any, epoch uint64) (protocol.RuntimeState, error) {
	var started, connected time.Time
	if r.observe != nil {
		started = time.Now()
	}
	host, err := r.owner.DesktopConnection(ctx)
	if err != nil {
		return protocol.RuntimeState{}, err
	}
	defer host.Close()
	if r.observe != nil {
		connected = time.Now()
	}
	var body protocol.BodySnapshot
	if err := host.Call(ctx, method, params, "BodySnapshot", &body); err != nil {
		return protocol.RuntimeState{}, err
	}
	if body.State.PrivacyEpoch != epoch {
		return protocol.RuntimeState{}, errors.New("PrivacyEpochChanged")
	}
	if r.observe != nil {
		described := time.Now()
		var metrics client.SnapshotReadMetrics
		metrics, err = host.ReadSnapshotMeasured(ctx, body, schema, result)
		r.observe(SnapshotMeasurement{
			Method: method, Bytes: body.TotalBytes, Elapsed: time.Since(started),
			Connect: connected.Sub(started), Descriptor: described.Sub(connected), Metrics: metrics, Complete: err == nil,
		})
	} else {
		err = host.ReadSnapshot(ctx, body, schema, result)
	}
	if err != nil {
		return protocol.RuntimeState{}, err
	}
	return body.State, nil
}

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
