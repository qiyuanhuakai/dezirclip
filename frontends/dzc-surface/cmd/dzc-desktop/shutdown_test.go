package main

import (
	"errors"
	"testing"
	"time"
)

func TestDesktopShutdownWaitsForChild(t *testing.T) {
	done := make(chan struct{})
	close(done)
	if err := waitDesktopExit(done, func() error { t.Fatal("terminated an exited child"); return nil }, time.Second); err != nil {
		t.Fatal(err)
	}
	done = make(chan struct{})
	if err := waitDesktopExit(done, func() error { close(done); return nil }, time.Millisecond); err != nil {
		t.Fatal(err)
	}
	if err := waitDesktopExit(make(chan struct{}), func() error { return errors.New("denied") }, time.Millisecond); err == nil {
		t.Fatal("ignored failed termination")
	}
	if err := waitDesktopExit(make(chan struct{}), func() error { return nil }, time.Millisecond); err == nil {
		t.Fatal("reported exit before confirmation")
	}
}
