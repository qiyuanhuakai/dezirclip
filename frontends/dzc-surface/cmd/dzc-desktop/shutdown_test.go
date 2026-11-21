package main

import (
	"errors"
	"io"
	"path/filepath"
	"strings"
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

func TestProfileRelaunchWaitsForApprovedParentExit(t *testing.T) {
	for _, invalid := range []string{"", "X", "RR"} {
		if acceptProfileRelaunch(strings.NewReader(invalid)) {
			t.Fatalf("accepted invalid relaunch signal %q", invalid)
		}
	}
	child, parent := io.Pipe()
	defer child.Close()
	defer parent.Close()
	accepted := make(chan bool, 1)
	go func() { accepted <- acceptProfileRelaunch(child) }()
	if _, err := io.WriteString(parent, "R"); err != nil {
		t.Fatal(err)
	}
	select {
	case <-accepted:
		t.Fatal("replacement started before the parent pipe closed")
	default:
	}
	if err := parent.Close(); err != nil {
		t.Fatal(err)
	}
	if !<-accepted {
		t.Fatal("replacement rejected the confirmed relaunch")
	}
	if _, err := startProfileReplacement(filepath.Join(t.TempDir(), "missing-desktop")); err == nil {
		t.Fatal("failed process launch was reported as success")
	}
}
