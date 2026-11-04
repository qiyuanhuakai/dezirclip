package main

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"testing"
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
