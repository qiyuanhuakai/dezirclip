package main

import (
	"errors"
	"testing"
)

func TestNativePanelRegistrationFailureRestoresSavedAndLiveState(t *testing.T) {
	conflict := errors.New("shortcut already registered")
	saved, live := false, false
	refreshes := 0
	err := changeNativePanel(false, true, func(enabled bool) error { saved = enabled; return nil }, func() error {
		refreshes++
		if saved {
			return conflict
		}
		live = saved
		return nil
	})
	if !errors.Is(err, conflict) || saved || live || refreshes != 2 {
		t.Fatalf("registration failure did not restore state: %v %v %v %d", err, saved, live, refreshes)
	}
}

func TestNativePanelStaleWriteDoesNotChangeRegistration(t *testing.T) {
	stale := errors.New("stale epoch")
	err := changeNativePanel(true, false, func(bool) error { return stale }, func() error {
		t.Fatal("registration changed after rejected write")
		return nil
	})
	if !errors.Is(err, stale) {
		t.Fatal(err)
	}
}
