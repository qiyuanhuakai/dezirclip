package main

import (
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestPrivacyStatusRetriesBusyAndBrokenTransport(t *testing.T) {
	for _, failure := range []error{&client.RemoteError{Code: protocol.WireErrorCodeBusy}, io.EOF} {
		calls := 0
		err := readPrivacyStatus(t.Context(), func() error {
			calls++
			if calls < 3 {
				return failure
			}
			return nil
		})
		if err != nil || calls != 3 {
			t.Fatalf("calls=%d err=%v", calls, err)
		}
	}
	calls := 0
	err := readPrivacyStatus(t.Context(), func() error { calls++; return io.EOF })
	if calls != 4 || err == nil || !strings.HasPrefix(err.Error(), "PrivacyStatusRetryable:") {
		t.Fatalf("unbounded or unclassified retry: %d %v", calls, err)
	}
}

func TestPrivacyStatusDoesNotRetryRejectedOrInvalidReads(t *testing.T) {
	for _, failure := range []error{client.ErrProtocol, context.Canceled, context.DeadlineExceeded,
		&client.RemoteError{Code: protocol.WireErrorCodePermissionDenied},
		&client.RemoteError{Code: protocol.WireErrorCodePrivacyBlocked},
		&client.RemoteError{Code: protocol.WireErrorCodePrivacyEpochChanged},
		&client.RemoteError{Code: protocol.WireErrorCode("session_expired")},
		errors.New("unclassified error")} {
		calls := 0
		err := readPrivacyStatus(t.Context(), func() error { calls++; return failure })
		if calls != 1 || err != failure {
			t.Fatalf("replayed rejection: %v", failure)
		}
	}
	ctx, cancel := context.WithCancel(t.Context())
	calls := 0
	err := readPrivacyStatus(ctx, func() error { calls++; cancel(); return io.EOF })
	if calls != 1 || err == nil {
		t.Fatal("cancelled status retried")
	}
	err = readPrivacyStatus(ctx, func() error { t.Fatal("read after cancellation"); return nil })
	if !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}

func TestPrivacyStatusDiagnosticsAreIndependentAndRedacted(t *testing.T) {
	d := &desktopDiagnostics{path: filepath.Join(t.TempDir(), "desktop.log"), role: "main"}
	d.eventRead(nil)
	d.statusRead(nil)
	d.statusRead(&client.RemoteError{Code: protocol.WireErrorCodeBusy})
	d.statusRead(errors.New("sensitive content"))
	d.statusRead(nil)
	d.statusRead(nil)
	raw, err := os.ReadFile(d.path)
	if err != nil {
		t.Fatal(err)
	}
	log := string(raw)
	if strings.Count(log, "\n") != 2 || strings.Contains(log, "sensitive") || strings.Contains(log, "events-") ||
		!strings.Contains(log, "remote-busy") || !strings.Contains(log, "privacy-status-recovered") {
		t.Fatal(log)
	}
}
