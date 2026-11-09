package main

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"testing"
	"time"
)

// A late response after cancellation must not complete the next settings action.
func TestCaptureReplyBelongsToRequest(t *testing.T) {
	input, output := io.Pipe()
	defer input.Close()
	defer output.Close()
	shell := DesktopShell{resident: residentBridge{output: output}}
	decoder := json.NewDecoder(input)
	start := func(ctx context.Context, full bool) (residentRequest, <-chan error) {
		done := make(chan error, 1)
		go func() { done <- shell.Capture(ctx, full, 7) }()
		var request residentRequest
		if err := decoder.Decode(&request); err != nil {
			t.Fatal(err)
		}
		if request.ID == 0 || request.Full != full || request.Epoch != 7 {
			t.Fatalf("wrong request: %+v", request)
		}
		return request, done
	}
	reply := func(id uint64, success bool) {
		data, err := json.Marshal(residentReply{ID: id, Success: success})
		if err != nil {
			t.Fatal(err)
		}
		shell.resident.accept(string(data))
	}
	ctx, cancel := context.WithCancel(t.Context())
	old, stopped := start(ctx, true)
	if err := shell.Capture(t.Context(), false, 7); err == nil {
		t.Fatal("parallel capture accepted")
	}
	cancel()
	if err := <-stopped; !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	current, done := start(t.Context(), false)
	if current.ID == old.ID {
		t.Fatal("capture ID reused")
	}
	reply(old.ID, true)
	reply(current.ID, false)
	if err := <-done; err == nil {
		t.Fatal("stale success masked a failure")
	}
	next, done := start(t.Context(), true)
	reply(next.ID, true)
	if err := <-done; err != nil {
		t.Fatal(err)
	}
}

func TestAppearanceDoesNotRejectForegroundAction(t *testing.T) {
	for _, pair := range [][2]string{{"appearance", "reset-settings"}, {"reset-settings", "appearance"}} {
		t.Run(pair[0], func(t *testing.T) {
			input, output := io.Pipe()
			defer input.Close()
			defer output.Close()
			bridge := residentBridge{output: output}
			ctx, cancel := context.WithTimeout(t.Context(), time.Second)
			defer cancel()
			decoder := json.NewDecoder(input)
			first := make(chan error, 1)
			go func() { first <- bridge.send(ctx, residentRequest{Operation: pair[0]}, time.Second) }()
			var pending residentRequest
			if err := decoder.Decode(&pending); err != nil {
				t.Fatal(err)
			}
			if err := bridge.send(ctx, residentRequest{Operation: "appearance"}, time.Millisecond); !errors.Is(err, context.DeadlineExceeded) {
				t.Fatalf("queued appearance did not honor its deadline: %v", err)
			}
			next := make(chan error, 1)
			go func() { next <- bridge.send(ctx, residentRequest{Operation: pair[1]}, time.Second) }()
			select {
			case err := <-next:
				t.Fatalf("background sync rejected an overlapping action: %v", err)
			case <-time.After(20 * time.Millisecond):
			}
			answer := func(id uint64) {
				encoded, _ := json.Marshal(residentReply{ID: id, Success: true})
				bridge.accept(string(encoded))
			}
			answer(pending.ID)
			if err := <-first; err != nil {
				t.Fatal(err)
			}
			var following residentRequest
			if err := decoder.Decode(&following); err != nil {
				t.Fatal(err)
			}
			if following.Operation != pair[1] || following.ID != pending.ID+1 {
				t.Fatal(following)
			}
			answer(following.ID)
			if err := <-next; err != nil {
				t.Fatal(err)
			}
		})
	}
}
