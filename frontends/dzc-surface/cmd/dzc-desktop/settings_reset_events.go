package main

import (
	"context"
	"time"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

// Keep the cursor across reconnects: a failed reset may complete on a later request.
func watchSettingsReset(ctx context.Context, owner *client.Client, reset, refresh func()) {
	state := owner.Hello().State
	for ctx.Err() == nil {
		poll, cancel := context.WithTimeout(ctx, 7*time.Second)
		host, err := owner.DesktopConnection(poll)
		cancel()
		if err == nil {
			err = followSettingsReset(ctx, host, &state, reset, refresh)
			host.Close()
		}
		if err == client.ErrProtocol {
			return
		}
		if err != nil {
			timer := time.NewTimer(time.Second)
			select {
			case <-ctx.Done():
				timer.Stop()
				return
			case <-timer.C:
			}
		}
	}
}

type settingsEventSource interface {
	Call(context.Context, protocol.Method, any, string, any) error
}

func followSettingsReset(ctx context.Context, host settingsEventSource, state *protocol.RuntimeState, reset, refresh func()) error {
	for ctx.Err() == nil {
		poll, cancel := context.WithTimeout(ctx, 3*time.Second)
		wait := uint16(1000)
		var result protocol.EventsResult
		err := host.Call(poll, protocol.MethodEventsSubscribe, protocol.SubscribeParams{AfterSequence: state.EventSequence, WaitMs: &wait}, "EventsResult", &result)
		cancel()
		if err != nil {
			return err
		}
		if result.State.ServiceInstanceId != state.ServiceInstanceId {
			return client.ErrProtocol
		}
		changed := false
		for _, event := range result.Events {
			if event.Event == protocol.EventKindServiceStateChanged && event.Data.Status != nil && *event.Data.Status == "application_settings_reset" {
				changed = true
			}
		}
		*state = result.State
		if changed && ctx.Err() == nil {
			reset()
		} else if result.ResyncRequired && ctx.Err() == nil {
			refresh()
		}
	}
	return ctx.Err()
}
