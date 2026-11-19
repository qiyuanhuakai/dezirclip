package main

import (
	"context"
	"errors"
	"slices"
	"testing"

	"github.com/qiyuanhuakai/dezirclip/clients/go/client"
	"github.com/qiyuanhuakai/dezirclip/clients/go/protocol"
)

func TestNativeStorageLimitMatchesServiceBounds(t *testing.T) {
	for input, expected := range map[string]string{
		"5": "50", "0052": "52", "99999": "99999", "100000": "99999",
		"9999999999999999999999999": "99999", "": "", "3.5": "", "-1": "",
	} {
		got, valid := nativeStorageLimit(input)
		if got != expected || valid != (expected != "") {
			t.Errorf("storage limit %q = %q, %t, want %q", input, got, valid, expected)
		}
	}
}

func TestNativePasteQueueEventsRetireReadsInFlight(t *testing.T) {
	count := nativeQueueCount{reading: true}
	token := count.events
	count.observe("paste_queue_changed:0")
	count.finishRead(token, 9, nil)
	if count.count != 0 || !count.known || count.reading || count.failed {
		t.Fatalf("stale status overrode the cleared queue: %+v", count)
	}
	token = count.events
	count.observe("paste_queue_changed:3")
	count.finishRead(token, 0, errors.New("stale read failure"))
	if count.count != 3 || !count.known || count.failed {
		t.Fatalf("stale read failure overrode the queued items: %+v", count)
	}
}

func TestQueueStatusUsesCompactReplyAndFallsBackForEarlierHosts(t *testing.T) {
	for _, legacy := range []bool{false, true} {
		var methods []protocol.Method
		params := protocol.LibraryStateParams{ServiceInstanceId: "fixture", ExpectedEpoch: 7}
		preferences := protocol.QueuePreferences{Sequential: true, PasteMethod: "ctrl_v"}
		status, err := readQueueStatus(context.Background(), func(_ context.Context, method protocol.Method, supplied any, _ string, result any) error {
			methods = append(methods, method)
			if supplied != params {
				t.Fatal("queue query did not preserve the original instance and epoch")
			}
			switch response := result.(type) {
			case *protocol.QueueStatus:
				if legacy {
					return &client.RemoteError{Code: protocol.WireErrorCodeInvalidRequest}
				}
				*response = protocol.QueueStatus{Count: 2, Revision: "fixture-revision", Preferences: preferences}
			case *protocol.QueueState:
				*response = protocol.QueueState{Ids: []int64{21, 22}, Revision: "fixture-revision", Preferences: preferences}
			default:
				t.Fatalf("unexpected response type %T", result)
			}
			return nil
		}, params)
		expected := []protocol.Method{protocol.MethodDesktopQueueStatus}
		if legacy {
			expected = append(expected, protocol.MethodDesktopQueueGet)
		}
		if err != nil || status.Count != 2 || status.Revision != "fixture-revision" || status.Preferences != preferences ||
			!slices.Equal(methods, expected) {
			t.Fatalf("legacy=%t methods=%v status=%+v error=%v", legacy, methods, status, err)
		}
	}
}
