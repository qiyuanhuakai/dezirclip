package main

import (
	"testing"

	"golang.org/x/sys/windows"
)

func TestWindowsEventPipeDisconnectsAreRecoverable(t *testing.T) {
	for _, failure := range []error{windows.ERROR_BROKEN_PIPE, windows.ERROR_NO_DATA, windows.ERROR_PIPE_NOT_CONNECTED} {
		if !eventTransportFailure(failure) {
			t.Fatalf("pipe disconnect was not recognized: %v", failure)
		}
	}
	for _, failure := range []error{windows.ERROR_ACCESS_DENIED, windows.ERROR_INVALID_DATA, windows.ERROR_OPERATION_ABORTED} {
		if eventTransportFailure(failure) {
			t.Fatalf("unrelated failure was replayed: %v", failure)
		}
	}
}
