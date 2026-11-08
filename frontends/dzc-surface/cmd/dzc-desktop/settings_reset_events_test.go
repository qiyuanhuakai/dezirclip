package main

import (
	"context"
	"errors"
	"testing"

	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

type resetEventFixture func(protocol.SubscribeParams, *protocol.EventsResult) error

func (f resetEventFixture) Call(_ context.Context, method protocol.Method, params any, schema string, output any) error {
	if method != protocol.MethodEventsSubscribe || schema != "EventsResult" {
		panic("unexpected request")
	}
	return f(params.(protocol.SubscribeParams), output.(*protocol.EventsResult))
}

func TestSettingsResetWatchRetainsCursorAfterBusyAndRebuildsOnCompletion(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	state := protocol.RuntimeState{ServiceInstanceId: "original", EventSequence: 12}
	busy := errors.New("Busy")
	failure := resetEventFixture(func(params protocol.SubscribeParams, _ *protocol.EventsResult) error {
		if params.AfterSequence != 12 || *params.WaitMs > 1000 {
			t.Fatal(params)
		}
		return busy
	})
	if err := followSettingsReset(ctx, failure, &state, func() { t.Fatal("premature rebuild") }, func() { t.Fatal("premature refresh") }); err != busy {
		t.Fatal(err)
	}
	completed := "application_settings_reset"
	resumed := resetEventFixture(func(params protocol.SubscribeParams, result *protocol.EventsResult) error {
		if params.AfterSequence != 12 {
			t.Fatal("lost recovery cursor", params)
		}
		result.State = protocol.RuntimeState{ServiceInstanceId: "original", EventSequence: 16}
		result.Events = []protocol.Event{{Event: protocol.EventKindServiceStateChanged, Data: protocol.EventData{Status: &completed}}}
		return nil
	})
	calls := 0
	err := followSettingsReset(ctx, resumed, &state, func() { calls++; cancel() }, func() { t.Fatal("unexpected refresh") })
	if !errors.Is(err, context.Canceled) || calls != 1 || state.EventSequence != 16 {
		t.Fatalf("state=%v calls=%d err=%v", state, calls, err)
	}
}

func TestSettingsEventGapRefreshesClaimsWithoutRestartingTheMainWindow(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	state := protocol.RuntimeState{ServiceInstanceId: "original", EventSequence: 12}
	source := resetEventFixture(func(_ protocol.SubscribeParams, result *protocol.EventsResult) error {
		result.State = protocol.RuntimeState{ServiceInstanceId: "original", EventSequence: 500}
		result.ResyncRequired = true
		return nil
	})
	refreshes := 0
	err := followSettingsReset(ctx, source, &state, func() { t.Fatal("history burst restarted main window") }, func() { refreshes++; cancel() })
	if !errors.Is(err, context.Canceled) || refreshes != 1 || state.EventSequence != 500 {
		t.Fatalf("state=%v refreshes=%d err=%v", state, refreshes, err)
	}
}
