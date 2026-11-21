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

func TestResidentBackgroundRequestDoesNotRejectForegroundAction(t *testing.T) {
	for _, pair := range [][2]string{
		{"appearance", "reset-settings"}, {"reset-settings", "appearance"},
		{"update-check", "install-update"}, {"install-update", "update-check"},
	} {
		t.Run(pair[0]+"/"+pair[1], func(t *testing.T) {
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

func TestResidentUpdateReplyRestoresCachedResult(t *testing.T) {
	input, output := io.Pipe()
	defer input.Close()
	defer output.Close()
	bridge := residentBridge{output: output}
	var response residentReply
	done := make(chan error, 1)
	go func() {
		request := residentRequest{Operation: "update-check", UpdateChannel: "preview"}
		done <- bridge.exchange(t.Context(), request, time.Second, &response)
	}()
	var request residentRequest
	if err := json.NewDecoder(input).Decode(&request); err != nil {
		t.Fatal(err)
	}
	if request.UpdateChannel != "preview" || request.UpdateNow {
		t.Fatalf("incorrect update read request: %+v", request)
	}
	now := time.Now()
	want := &nativeUpdateCheck{
		Result:  nativeUpdateResult{Channel: "preview", CurrentVersion: "1.0.1-beta.1", ChannelVersion: "1.0.1", Status: "available"},
		Checked: now,
	}
	progress := residentUpdateStatus{Progress: nativeInstallProgress{Phase: updateDownloading, Downloaded: 42, Total: 100}}
	payload, err := json.Marshal(residentReply{ID: request.ID, Success: true, UpdateCheck: want, UpdateStatus: &progress})
	if err != nil {
		t.Fatal(err)
	}
	reopened := nativeUpdater{channel: "preview"}
	status := bridge.accept(string(payload)).UpdateStatus
	if status == nil {
		t.Fatal("a reopened main window did not receive the resident install status")
	}
	reopened.applyInstallStatus(*status)
	if !reopened.installing() || reopened.install.Downloaded != 42 || reopened.install.Total != 100 {
		t.Fatal("resident installation progress was not restored")
	}
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	got := response.UpdateCheck
	if !got.validFor("preview", "1.0.1-beta.1", now) || got.Result.ChannelVersion != "1.0.1" {
		t.Fatalf("valid update result did not survive the bridge: %+v", got)
	}
	if got.validFor("stable", "1.0.1-beta.1", now) || got.validFor("preview", "1.0.1", now) ||
		got.validFor("preview", "1.0.1-beta.1", now.Add(nativeUpdateInterval)) ||
		got.validFor("preview", "1.0.1-beta.1", now.Add(-time.Second)) {
		t.Fatal("restored a check outside its version, channel or time window")
	}
	event, err := json.Marshal(residentReply{UpdateStatus: &residentUpdateStatus{Failure: "update_install_failed"}})
	if err != nil {
		t.Fatal(err)
	}
	if status = bridge.accept(string(event)).UpdateStatus; status == nil {
		t.Fatal("the resident lost a failure after the original request disconnected")
	}
	reopened.applyInstallStatus(*status)
	// Both messages are scheduled on the main window when it reopens.
	// The cached version check can arrive after the failed install.
	reopened.applyCheck(got)
	if reopened.result.ChannelVersion != "1.0.1" || reopened.installing() ||
		reopened.installErr == nil || reopened.installErr.Error() != "update_install_failed" {
		t.Fatal("the reopened window did not clear progress and report the failure")
	}
}
