package application

import (
	"context"
	"errors"
	"io"
	"strings"
	"testing"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestPrivacyStatusRejectsMismatchedHostAndEpoch(t *testing.T) {
	current := protocol.ServiceStatus{
		State:   protocol.RuntimeState{ServiceInstanceId: "authenticated", PrivacyEpoch: 3},
		Privacy: protocol.PrivacyState{PrivacyEpoch: 3},
	}
	for _, status := range []protocol.ServiceStatus{
		{State: protocol.RuntimeState{ServiceInstanceId: "retired", PrivacyEpoch: 3}, Privacy: current.Privacy},
		{State: current.State, Privacy: protocol.PrivacyState{PrivacyEpoch: 2}},
	} {
		calls := 0
		var observed error
		got, err := readServiceStatus(t.Context(), "authenticated", func(result *protocol.ServiceStatus) error {
			calls++
			*result = status
			return nil
		}, func(err error) { observed = err })
		if !errors.Is(err, client.ErrProtocol) || got.State.ServiceInstanceId != "" || calls != 1 || !errors.Is(observed, client.ErrProtocol) {
			t.Fatalf("invalid status was retained or retried: %+v, calls=%d, err=%v, observed=%v", got, calls, err, observed)
		}
	}
	got, err := readServiceStatus(t.Context(), "authenticated", func(result *protocol.ServiceStatus) error {
		*result = current
		return nil
	}, nil)
	if err != nil || got != current {
		t.Fatalf("matching status was rejected: %+v %v", got, err)
	}
}

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
