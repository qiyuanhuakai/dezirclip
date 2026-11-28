package runtime

import (
	"context"
	"net"
	"sync"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

// EventStream owns the connection used by one desktop window to poll Host events.
// Its callbacks run in read order. Foreground actions use separate connections.
type EventStream struct {
	mu          sync.Mutex
	dial        func(context.Context) (*client.Client, error)
	onReconnect func(error)
	onRead      func(context.Context, error)
	host        *client.Client
	closed      bool
}

func NewEventStream(dial func(context.Context) (*client.Client, error), onReconnect func(error), onRead func(context.Context, error)) *EventStream {
	return &EventStream{dial: dial, onReconnect: onReconnect, onRead: onRead}
}

func (s *EventStream) Read(ctx context.Context, cursor *uint64) (protocol.EventsResult, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return protocol.EventsResult{}, net.ErrClosed
	}

	var events protocol.EventsResult
	var lastReadError error
	err := readEventStream(ctx, func(fresh bool) error {
		if fresh && s.host != nil {
			s.closeConnection()
			if s.onReconnect != nil {
				s.onReconnect(lastReadError)
			}
		}
		if s.host == nil {
			host, err := s.dial(ctx)
			if err != nil {
				return err
			}
			s.host = host
		}
		events = protocol.EventsResult{}
		if cursor == nil {
			// Begin a new presentation from the current event sequence.
			lastReadError = s.host.Call(ctx, protocol.MethodHealth, struct{}{}, "RuntimeState", &events.State)
			events.Events = []protocol.Event{}
		} else {
			wait := uint16(1000)
			params := protocol.SubscribeParams{AfterSequence: *cursor, WaitMs: &wait}
			lastReadError = s.host.Call(ctx, protocol.MethodEventsSubscribe, params, "EventsResult", &events)
		}
		return lastReadError
	})
	if err != nil {
		s.closeConnection()
	}
	if s.onRead != nil {
		s.onRead(ctx, err)
	}
	return events, err
}

func (s *EventStream) Close() {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.closed = true
	s.closeConnection()
}

func (s *EventStream) closeConnection() {
	if s.host != nil {
		s.host.Close()
		s.host = nil
	}
}
