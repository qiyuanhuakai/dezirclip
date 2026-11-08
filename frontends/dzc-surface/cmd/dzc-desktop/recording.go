package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

type recordingEvent struct {
	Recording bool   `json:"recording_event"`
	Hotkey    string `json:"hotkey,omitempty"`
	Cancelled bool   `json:"cancelled,omitempty"`
}

var hotkeyRecorded = mygo.NewEvent[string]("hotkey-recorded")
var recordingCancelled = mygo.NewEvent[bool]("recording-cancelled")

// All fields belong to the native main thread. The poller returns through RunOnMain.
type shortcutRecording struct {
	post     func(func())
	stopPoll chan struct{}
	pollDone chan struct{}
	host     *client.Client
	cancel   context.CancelFunc
	emit     func(recordingEvent)
}

func (r *shortcutRecording) offer(hotkey string) bool {
	if r.host == nil {
		return false
	}
	r.emit(recordingEvent{Recording: true, Hotkey: hotkey})
	return true
}

func (r *shortcutRecording) stop() error {
	if r.host == nil {
		return nil
	}
	host := r.host
	r.host = nil
	close(r.stopPoll)
	defer r.cancel()
	defer host.Close()
	select {
	case <-r.pollDone:
	case <-time.After(2 * time.Second):
		return errors.New("RecordingStopTimeout")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	var result protocol.OperationResult
	err := host.Call(ctx, protocol.MethodDesktopHotkeyRecording, protocol.RecordingParams{
		ServiceInstanceId: host.Hello().State.ServiceInstanceId, Enabled: false,
	}, "OperationResult", &result)
	if err != nil {
		return fmt.Errorf("RecordingStop: %w", err)
	}
	return err
}

func (r *shortcutRecording) abort() {
	if r.host == nil {
		return
	}
	_ = r.stop()
	r.emit(recordingEvent{Recording: true, Cancelled: true})
}

func (r *shortcutRecording) start(owner *client.Client, epoch uint64, emit func(recordingEvent)) error {
	if r.host != nil {
		return nil
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	host, err := owner.DesktopConnection(ctx)
	if err != nil {
		return fmt.Errorf("RecordingConnect: %w", err)
	}
	var result protocol.OperationResult
	err = host.Call(ctx, protocol.MethodDesktopHotkeyRecording, protocol.RecordingParams{
		ServiceInstanceId: owner.Hello().State.ServiceInstanceId, ExpectedEpoch: epoch, Enabled: true,
	}, "OperationResult", &result)
	if err != nil || !result.Performed {
		host.Close()
		if err == nil {
			err = client.ErrProtocol
		}
		return fmt.Errorf("RecordingStart: %w", err)
	}
	r.host, r.emit = host, emit
	poll, stop := context.WithTimeout(context.Background(), 45*time.Second)
	r.cancel = stop
	context.AfterFunc(poll, func() {
		r.post(func() {
			if r.host == host {
				r.abort()
			}
		})
	})
	stopPoll, pollDone := make(chan struct{}), make(chan struct{})
	r.stopPoll, r.pollDone = stopPoll, pollDone
	go func() {
		recorded := ""
		defer func() {
			close(pollDone)
			r.post(func() {
				if r.host == host {
					if recorded != "" {
						r.offer(recorded)
					} else {
						r.abort()
					}
				}
			})
		}()
		sequence := host.Hello().State.EventSequence
		wait := uint16(500)
		for poll.Err() == nil {
			select {
			case <-stopPoll:
				return
			default:
			}
			var events protocol.EventsResult
			if err := host.Call(poll, protocol.MethodEventsSubscribe, protocol.SubscribeParams{AfterSequence: sequence, WaitMs: &wait}, "EventsResult", &events); err != nil {
				return
			}
			if events.ResyncRequired || events.State.PrivacyEpoch != epoch {
				return
			}
			sequence = events.State.EventSequence
			for _, event := range events.Events {
				if event.Data.Status == nil {
					continue
				}
				if chord, ok := strings.CutPrefix(*event.Data.Status, "hotkey_recorded:"); ok && chord != "" {
					recorded = chord
					return
				}
			}
		}
	}()
	return nil
}

func (s *DesktopShell) SetRecordingMode(ctx context.Context, enabled bool, epoch uint64) error {
	w := mygo.CallerWindow(ctx)
	if enabled && (w == nil || !w.IsFocused()) {
		return errors.New("RecordingRequiresFocusedWindow")
	}
	return s.resident.send(ctx, residentRequest{Operation: "recording", Disabled: !enabled, Epoch: epoch}, 6*time.Second)
}

func acceptRecordingEvent(w *mygo.Window, message string) bool {
	var event recordingEvent
	if json.Unmarshal([]byte(message), &event) != nil || !event.Recording {
		return false
	}
	mygo.RunOnMain(func() {
		if w.IsDestroyed() {
			return
		}
		if event.Cancelled {
			recordingCancelled.Emit(w, true)
		} else if event.Hotkey != "" {
			hotkeyRecorded.Emit(w, event.Hotkey)
		}
	})
	return true
}
